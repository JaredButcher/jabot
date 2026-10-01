//! Handler tests: the real handlers against mocked storage and Discord.

use std::sync::{Arc, Mutex};

use mockall::Sequence;
use mockall::predicate::eq;
use rand::SeedableRng;
use rand::rngs::StdRng;
use serenity::all::UserId;

use super::model::{Event, EventId, EventStatus, Participant};
use super::repo::MockSecretSantaRepo;
use super::{SecretSanta, components, text};
use crate::framework::testing::{any_users, content, ctx, followup_content, is_ephemeral, json};
use crate::framework::{
    CommandRequest, ComponentRequest, DmError, Feature, FeatureError, InteractionCtx,
    MockDiscordApi, MockResponder, MockUserRepo, ModalRequest,
};
use serenity::all::CommandDataOptionValue;

const EVENT: EventId = EventId(5);

fn user(id: u64) -> UserId {
    UserId::new(id)
}

fn host() -> UserId {
    user(1)
}

fn event(status: EventStatus) -> Event {
    Event {
        id: EVENT,
        name: "Party".into(),
        description: Some("Bring snacks".into()),
        host: host(),
        status,
    }
}

fn participants(ids: &[u64]) -> Vec<Participant> {
    ids.iter()
        .map(|&id| Participant {
            user: user(id),
            assignee: None,
        })
        .collect()
}

fn santa(repo: MockSecretSantaRepo) -> SecretSanta {
    SecretSanta::new(Arc::new(repo))
}

fn repo_with(event: Event, members: &[u64]) -> MockSecretSantaRepo {
    let members = participants(members);
    let mut repo = MockSecretSantaRepo::new();
    repo.expect_get_event()
        .with(eq(EVENT))
        .returning(move |_| Ok(Some(event.clone())));
    repo.expect_participants()
        .with(eq(EVENT))
        .returning(move |_| Ok(members.clone()));
    repo
}

/// A responder that expects exactly one ephemeral message with this content.
fn replies(expected: &'static str) -> MockResponder {
    let mut responder = MockResponder::new();
    responder
        .expect_respond()
        .withf(move |r| content(r) == expected && is_ephemeral(r))
        .times(1)
        .returning(|_| Ok(()));
    responder
}

type DmLog = Arc<Mutex<Vec<(UserId, String)>>>;

/// Collects every DM sent, in order.
fn recording_discord() -> (MockDiscordApi, DmLog) {
    let sent = Arc::new(Mutex::new(vec![]));
    let log = sent.clone();
    let mut discord = MockDiscordApi::new();
    discord.expect_send_dm().returning(move |to, text| {
        log.lock().unwrap().push((to, text));
        Ok(())
    });
    (discord, sent)
}

/// The handler refused with this user-facing message (the registry shows it ephemerally).
fn assert_refused(result: Result<(), impl Into<FeatureError>>, expected: &str) {
    match result.map_err(Into::into) {
        Err(FeatureError::User(message)) => assert_eq!(message, expected),
        other => panic!("expected refusal {expected:?}, got {other:?}"),
    }
}

fn plain_ctx(responder: MockResponder) -> InteractionCtx {
    ctx(responder, MockDiscordApi::new(), MockUserRepo::new())
}

// --- start ---

#[tokio::test]
async fn start_rejects_non_host() {
    let mut repo = repo_with(event(EventStatus::PreRun), &[1, 2]);
    repo.expect_start_event().times(0);
    let ss = santa(repo);

    let ctx = plain_ctx(MockResponder::new());
    let result = ss
        .on_component(&ctx, ComponentRequest::button(user(2), "ss:start:5"))
        .await;
    assert_refused(result, text::NOT_HOST);
}

#[tokio::test]
async fn start_requires_two_participants() {
    let mut repo = repo_with(event(EventStatus::PreRun), &[1]);
    repo.expect_start_event().times(0);
    let ss = santa(repo);

    let ctx = plain_ctx(MockResponder::new());
    let result = ss
        .on_component(&ctx, ComponentRequest::button(host(), "ss:start:5"))
        .await;
    assert_refused(result, text::NOT_ENOUGH_PARTICIPANTS);
}

#[tokio::test]
async fn start_saves_assignments_then_replies_then_dms_each_santa() {
    let mut seq = Sequence::new();
    let saved = Arc::new(Mutex::new(vec![]));
    let mut repo = repo_with(event(EventStatus::PreRun), &[1, 2, 3]);
    let log = saved.clone();
    repo.expect_start_event()
        .withf(|id, _| *id == EVENT)
        .times(1)
        .in_sequence(&mut seq)
        .returning(move |_, assignments| {
            *log.lock().unwrap() = assignments.to_vec();
            Ok(true)
        });
    let mut responder = MockResponder::new();
    responder
        .expect_respond()
        .withf(|r| content(r) == text::STARTED)
        .times(1)
        .in_sequence(&mut seq)
        .returning(|_| Ok(()));
    let dms = Arc::new(Mutex::new(vec![]));
    let dm_log = dms.clone();
    let mut discord = MockDiscordApi::new();
    discord
        .expect_send_dm()
        .times(3)
        .in_sequence(&mut seq)
        .returning(move |to, text| {
            dm_log.lock().unwrap().push((to, text));
            Ok(())
        });
    let ss = santa(repo);
    let ctx = ctx(responder, discord, MockUserRepo::new());

    let mut rng = StdRng::seed_from_u64(7);
    components::start(&ss, &ctx, EVENT, host(), &mut rng)
        .await
        .unwrap();

    let saved = saved.lock().unwrap().clone();
    let mut santas: Vec<_> = saved.iter().map(|(s, _)| *s).collect();
    let mut recipients: Vec<_> = saved.iter().map(|(_, r)| *r).collect();
    santas.sort();
    recipients.sort();
    assert_eq!(santas, [user(1), user(2), user(3)]);
    assert_eq!(recipients, [user(1), user(2), user(3)]);
    assert!(saved.iter().all(|(s, r)| s != r), "nobody draws themselves");
    for (to, text) in dms.lock().unwrap().iter() {
        let (_, recipient) = saved.iter().find(|(s, _)| s == to).unwrap();
        assert!(text.contains(&format!("<@{recipient}>")), "{text}");
    }
}

/// B5: if the event was started in the meantime (double click), nobody is re-notified.
#[tokio::test]
async fn start_already_started_sends_no_dms() {
    let mut repo = repo_with(event(EventStatus::PreRun), &[1, 2]);
    repo.expect_start_event().returning(|_, _| Ok(false));
    let ss = santa(repo);

    let ctx = plain_ctx(MockResponder::new());
    let mut rng = StdRng::seed_from_u64(1);
    let result = components::start(&ss, &ctx, EVENT, host(), &mut rng).await;
    assert_refused(result, text::ALREADY_STARTED);
}

// --- end / cancel ---

#[tokio::test]
async fn end_running_event_notifies_everyone() {
    let mut repo = repo_with(event(EventStatus::Running), &[1, 2]);
    repo.expect_transition()
        .withf(|id, from, to| {
            *id == EVENT && from == [EventStatus::Running] && *to == EventStatus::Finished
        })
        .times(1)
        .returning(|_, _, _| Ok(true));
    let (discord, sent) = recording_discord();
    let ss = santa(repo);

    let ctx = ctx(replies(text::ENDED), discord, MockUserRepo::new());
    ss.on_component(&ctx, ComponentRequest::button(host(), "ss:end:5"))
        .await
        .unwrap();

    let recipients: Vec<_> = sent.lock().unwrap().iter().map(|(to, _)| *to).collect();
    assert_eq!(recipients, [user(1), user(2)]);
}

#[tokio::test]
async fn end_requires_running_event() {
    let mut repo = repo_with(event(EventStatus::PreRun), &[1, 2]);
    repo.expect_transition().times(0);
    let ss = santa(repo);

    let ctx = plain_ctx(MockResponder::new());
    let result = ss
        .on_component(&ctx, ComponentRequest::button(host(), "ss:end:5"))
        .await;
    assert_refused(result, text::NOT_RUNNING);
}

#[tokio::test]
async fn cancel_running_event_replies_then_dms_participants() {
    let mut repo = repo_with(event(EventStatus::Running), &[1, 2]);
    repo.expect_transition()
        .withf(|_, from, to| from == [EventStatus::Running] && *to == EventStatus::Finished)
        .times(1)
        .returning(|_, _, _| Ok(true));

    let mut seq = Sequence::new();
    let mut responder = MockResponder::new();
    responder
        .expect_respond()
        .withf(|r| content(r) == text::CANCELED)
        .times(1)
        .in_sequence(&mut seq)
        .returning(|_| Ok(()));
    let mut discord = MockDiscordApi::new();
    discord
        .expect_send_dm()
        .withf(|_, text| text.contains("has been canceled"))
        .times(2)
        .in_sequence(&mut seq)
        .returning(|_, _| Ok(()));
    let ss = santa(repo);

    let ctx = ctx(responder, discord, MockUserRepo::new());
    ss.on_component(&ctx, ComponentRequest::button(host(), "ss:cancel:5"))
        .await
        .unwrap();
}

/// B6: one participant with DMs closed doesn't stop the others being told, and the host
/// hears who was missed.
#[tokio::test]
async fn failed_dm_is_reported_and_others_still_notified() {
    let mut repo = repo_with(event(EventStatus::Running), &[1, 2, 3]);
    repo.expect_transition().returning(|_, _, _| Ok(true));
    let delivered = Arc::new(Mutex::new(vec![]));
    let log = delivered.clone();
    let mut discord = MockDiscordApi::new();
    discord.expect_send_dm().times(3).returning(move |to, _| {
        if to == user(2) {
            return Err(DmError::Closed);
        }
        log.lock().unwrap().push(to);
        Ok(())
    });
    let mut responder = replies(text::CANCELED);
    responder
        .expect_followup()
        .withf(|f| followup_content(f).contains("Couldn't DM: <@2>"))
        .times(1)
        .returning(|_| Ok(()));
    let ss = santa(repo);

    let ctx = ctx(responder, discord, MockUserRepo::new());
    ss.on_component(&ctx, ComponentRequest::button(host(), "ss:cancel:5"))
        .await
        .unwrap();

    assert_eq!(*delivered.lock().unwrap(), [user(1), user(3)]);
}

#[tokio::test]
async fn cancel_preparing_event_sends_no_dms() {
    let mut repo = repo_with(event(EventStatus::PreRun), &[1, 2]);
    repo.expect_transition()
        .withf(|_, from, _| from == [EventStatus::PreRun])
        .returning(|_, _, _| Ok(true));
    let ss = santa(repo);

    let ctx = plain_ctx(replies(text::CANCELED));
    ss.on_component(&ctx, ComponentRequest::button(host(), "ss:cancel:5"))
        .await
        .unwrap();
}

#[tokio::test]
async fn cancel_finished_event_is_rejected() {
    let mut repo = repo_with(event(EventStatus::Finished), &[1, 2]);
    repo.expect_transition().times(0);
    let ss = santa(repo);

    let ctx = plain_ctx(MockResponder::new());
    let result = ss
        .on_component(&ctx, ComponentRequest::button(host(), "ss:cancel:5"))
        .await;
    assert_refused(result, text::ALREADY_FINISHED);
}

// --- participant picker ---

#[tokio::test]
async fn picker_adds_and_removes_then_invites_new_participants() {
    let mut seq = Sequence::new();
    let mut repo = repo_with(event(EventStatus::PreRun), &[1, 2, 3]);
    let mut users = MockUserRepo::new();
    users
        .expect_ensure()
        .withf(|ids| ids == [user(4)])
        .times(1)
        .in_sequence(&mut seq)
        .returning(|_| Ok(()));
    repo.expect_set_participants()
        .withf(|id, add, remove| *id == EVENT && add == [user(4)] && remove == [user(2)])
        .times(1)
        .in_sequence(&mut seq)
        .returning(|_, _, _| Ok(()));
    let mut responder = MockResponder::new();
    responder
        .expect_respond()
        .withf(|r| json(r)["type"] == 6) // Acknowledge
        .times(1)
        .in_sequence(&mut seq)
        .returning(|_| Ok(()));
    let mut discord = MockDiscordApi::new();
    discord
        .expect_send_dm()
        .withf(|to, text| *to == user(4) && text.contains("invited"))
        .times(1)
        .in_sequence(&mut seq)
        .returning(|_, _| Ok(()));
    let ss = santa(repo);

    let ctx = ctx(responder, discord, users);
    let selected = vec![user(1), user(3), user(4)];
    ss.on_component(
        &ctx,
        ComponentRequest::user_select(host(), "ss:participants:5", selected),
    )
    .await
    .unwrap();
}

/// B3: only the host may change who is in the event.
#[tokio::test]
async fn picker_rejects_non_host() {
    let mut repo = repo_with(event(EventStatus::PreRun), &[1, 2]);
    repo.expect_set_participants().times(0);
    let ss = santa(repo);

    let ctx = plain_ctx(MockResponder::new());
    let result = ss
        .on_component(
            &ctx,
            ComponentRequest::user_select(user(2), "ss:participants:5", vec![user(2)]),
        )
        .await;
    assert_refused(result, text::NOT_HOST);
}

/// B3: a picker for a deleted event is refused rather than crashing.
#[tokio::test]
async fn picker_for_missing_event_is_refused() {
    let mut repo = MockSecretSantaRepo::new();
    repo.expect_get_event().returning(|_| Ok(None));
    repo.expect_set_participants().times(0);
    let ss = santa(repo);

    let ctx = plain_ctx(MockResponder::new());
    let result = ss
        .on_component(
            &ctx,
            ComponentRequest::user_select(host(), "ss:participants:5", vec![user(2)]),
        )
        .await;
    assert_refused(result, text::EVENT_NOT_FOUND);
}

/// B4: the host stays in the event even if they deselect themselves.
#[tokio::test]
async fn picker_never_removes_the_host() {
    let mut repo = repo_with(event(EventStatus::PreRun), &[1, 2]);
    repo.expect_set_participants()
        .withf(|_, add, remove| add.is_empty() && remove == [user(2)])
        .times(1)
        .returning(|_, _, _| Ok(()));
    let mut responder = MockResponder::new();
    responder.expect_respond().returning(|_| Ok(()));
    let ss = santa(repo);

    let ctx = ctx(responder, MockDiscordApi::new(), any_users());
    ss.on_component(
        &ctx,
        ComponentRequest::user_select(host(), "ss:participants:5", vec![]),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn picker_is_locked_after_start() {
    let mut repo = repo_with(event(EventStatus::Running), &[1, 2]);
    repo.expect_set_participants().times(0);
    let ss = santa(repo);

    let ctx = plain_ctx(MockResponder::new());
    let result = ss
        .on_component(
            &ctx,
            ComponentRequest::user_select(host(), "ss:participants:5", vec![user(1)]),
        )
        .await;
    assert_refused(result, text::PARTICIPANTS_LOCKED);
}

// --- forms ---

#[tokio::test]
async fn create_form_creates_event() {
    let mut repo = MockSecretSantaRepo::new();
    repo.expect_count_active_hosted().returning(|_| Ok(0));
    repo.expect_create_event()
        .withf(|h, name, desc| *h == host() && name == "Party" && desc == "Snacks")
        .times(1)
        .returning(|_, _, _| Ok(EventId(9)));
    let mut responder = MockResponder::new();
    responder
        .expect_respond()
        .withf(|r| content(r).contains("**Id:** 9") && is_ephemeral(r))
        .times(1)
        .returning(|_| Ok(()));
    let ss = santa(repo);

    let form = ModalRequest::new(
        host(),
        "ss:create",
        [
            (text::FIELD_NAME, "Party"),
            (text::FIELD_DESCRIPTION, "Snacks"),
        ],
    );
    ss.on_modal(&plain_ctx(responder), form).await.unwrap();
}

#[tokio::test]
async fn create_form_respects_event_limit() {
    let mut repo = MockSecretSantaRepo::new();
    repo.expect_count_active_hosted().returning(|_| Ok(32));
    repo.expect_create_event().times(0);
    let mut responder = MockResponder::new();
    responder
        .expect_respond()
        .withf(|r| content(r).contains("limit of 32"))
        .times(1)
        .returning(|_| Ok(()));
    let ss = santa(repo);

    let form = ModalRequest::new(host(), "ss:create", [(text::FIELD_NAME, "Party")]);
    ss.on_modal(&plain_ctx(responder), form).await.unwrap();
}

#[tokio::test]
async fn edit_form_by_non_host_changes_nothing() {
    let mut repo = repo_with(event(EventStatus::PreRun), &[1, 2]);
    repo.expect_update_event().times(0);
    let ss = santa(repo);

    let form = ModalRequest::new(user(2), "ss:edit:5", [(text::FIELD_NAME, "Mine now")]);
    let result = ss.on_modal(&plain_ctx(MockResponder::new()), form).await;
    assert_refused(result, text::NOT_HOST);
}

#[tokio::test]
async fn edit_form_updates_event() {
    let mut repo = repo_with(event(EventStatus::PreRun), &[1, 2]);
    repo.expect_update_event()
        .withf(|id, name, desc| *id == EVENT && name == "New" && desc.as_deref() == Some(""))
        .times(1)
        .returning(|_, _, _| Ok(()));
    let mut responder = MockResponder::new();
    responder
        .expect_respond()
        .withf(|r| content(r) == "Event 'New' updated successfully")
        .times(1)
        .returning(|_| Ok(()));
    let ss = santa(repo);

    let form = ModalRequest::new(
        host(),
        "ss:edit:5",
        [(text::FIELD_NAME, "New"), (text::FIELD_DESCRIPTION, "")],
    );
    ss.on_modal(&plain_ctx(responder), form).await.unwrap();
}

// --- commands ---

fn ss_command(user: UserId, subcommand: &str) -> CommandRequest {
    CommandRequest::new(user, text::CMD_SS).subcommand(subcommand)
}

#[tokio::test]
async fn info_requires_participation() {
    let ss = santa(repo_with(event(EventStatus::Running), &[1, 2]));

    let req = ss_command(user(9), text::CMD_INFO)
        .option(text::OPT_EVENT_ID, CommandDataOptionValue::Integer(5));
    let result = ss.on_command(&plain_ctx(MockResponder::new()), req).await;
    assert_refused(result, text::NOT_PARTICIPANT);
}

#[tokio::test]
async fn info_shows_event_to_participant() {
    let ss = santa(repo_with(event(EventStatus::Running), &[1, 2]));
    let mut responder = MockResponder::new();
    responder
        .expect_respond()
        .withf(|r| content(r).contains("**Name:** Party"))
        .times(1)
        .returning(|_| Ok(()));

    let req = ss_command(user(2), text::CMD_INFO)
        .option(text::OPT_EVENT_ID, CommandDataOptionValue::Integer(5));
    ss.on_command(&plain_ctx(responder), req).await.unwrap();
}

#[tokio::test]
async fn create_command_without_id_opens_form() {
    let ss = santa(MockSecretSantaRepo::new());
    let mut responder = MockResponder::new();
    responder
        .expect_respond()
        .withf(|r| json(r)["data"]["custom_id"] == "ss:create")
        .times(1)
        .returning(|_| Ok(()));

    ss.on_command(&plain_ctx(responder), ss_command(host(), text::CMD_CREATE))
        .await
        .unwrap();
}

#[tokio::test]
/// B7: hosts are shown as mentions, so listing makes no Discord API calls.
async fn list_shows_users_events_without_api_calls() {
    let mut repo = MockSecretSantaRepo::new();
    repo.expect_events_for_user()
        .with(eq(user(2)))
        .returning(|_| Ok(vec![event(EventStatus::Running)]));
    let mut discord = MockDiscordApi::new();
    discord.expect_user_name().times(0);
    let mut responder = MockResponder::new();
    responder
        .expect_respond()
        .withf(|r| content(r).contains("**Name:** Party") && content(r).contains("**Host:** <@1>"))
        .times(1)
        .returning(|_| Ok(()));
    let ss = santa(repo);

    let ctx = ctx(responder, discord, any_users());
    ss.on_command(&ctx, ss_command(user(2), text::CMD_LIST))
        .await
        .unwrap();
}
