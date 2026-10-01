//! Handler tests: the real handlers against mocked storage and Discord.

use std::sync::Arc;

use mockall::Sequence;
use mockall::predicate::eq;
use serenity::all::UserId;

use super::model::{StoredToken, TokenId};
use super::repo::MockTellRepo;
use super::{Tell, TellConfig, text};
use crate::framework::testing::{any_users, content, ctx, followup_content, is_ephemeral, json};
use crate::framework::{
    CommandRequest, ComponentRequest, DmError, Feature, FeatureError, MockDiscordApi, MockResponder,
};

const TOKEN: &str = "tell_abc";

fn user() -> UserId {
    UserId::new(1)
}

fn config() -> TellConfig {
    TellConfig {
        url: "https://example.com/jabot/tell".into(),
        lan_url: None,
    }
}

fn stored(created: bool) -> StoredToken {
    StoredToken {
        id: TokenId(7),
        token: TOKEN.into(),
        created,
    }
}

fn tell(repo: MockTellRepo, config: TellConfig) -> Tell {
    Tell::new(Arc::new(repo), config)
}

fn repo_returning(token: StoredToken) -> MockTellRepo {
    let mut repo = MockTellRepo::new();
    repo.expect_get_or_create()
        .withf(|user_id, candidate| *user_id == user() && candidate.starts_with("tell_"))
        .times(1)
        .returning(move |_, _| Ok(token.clone()));
    repo
}

/// Records the `/tell` reply and lets the test inspect it afterwards.
fn capturing_responder(
    seq: &mut Sequence,
) -> (
    MockResponder,
    Arc<std::sync::Mutex<Option<serde_json::Value>>>,
) {
    let reply = Arc::new(std::sync::Mutex::new(None));
    let stored = reply.clone();
    let mut responder = MockResponder::new();
    responder
        .expect_respond()
        .times(1)
        .in_sequence(seq)
        .returning(move |response| {
            assert!(is_ephemeral(&response));
            *stored.lock().unwrap() = Some(json(&response));
            Ok(())
        });
    (responder, reply)
}

async fn run_tell(
    feature: &Tell,
    responder: MockResponder,
    discord: MockDiscordApi,
) -> Result<(), FeatureError> {
    let ctx = ctx(responder, discord, any_users());
    feature
        .on_command(&ctx, CommandRequest::new(user(), text::CMD_TELL))
        .await
}

#[tokio::test]
async fn new_token_gets_reply_then_test_dm() {
    let mut seq = Sequence::new();
    let (responder, reply) = capturing_responder(&mut seq);
    let mut discord = MockDiscordApi::new();
    discord
        .expect_send_dm()
        .with(eq(user()), eq(text::TEST_DM.to_string()))
        .times(1)
        .in_sequence(&mut seq)
        .returning(|_, _| Ok(()));

    run_tell(
        &tell(repo_returning(stored(true)), config()),
        responder,
        discord,
    )
    .await
    .unwrap();

    let reply = reply.lock().unwrap().clone().unwrap();
    let button = &reply["data"]["components"][0]["components"][0];
    assert_eq!(button["custom_id"], "tell:revoke:7");
    assert_eq!(button["label"], text::BTN_REVOKE);
}

#[test]
fn reply_has_ready_to_run_commands() {
    let response = super::views::tell_reply(&config(), &stored(false));

    assert_eq!(
        content(&response),
        r#"Your tell token: `tell_abc`
Anyone with it can DM you through me, so keep it private.

Send yourself a message:
```sh
curl -sS --fail-with-body https://example.com/jabot/tell \
     --json '{"token": "tell_abc", "message": "Task finished"}'
```
Shell helper (needs `jq`), e.g. `long_task; tell "long_task exited with $?"`:
```sh
tell() { jq -nc --arg token tell_abc --arg message "${*:-done}" '$ARGS.named' | curl -sS --fail-with-body https://example.com/jabot/tell --json @-; }
```"#
    );
}

#[test]
fn reply_mentions_lan_url_when_configured() {
    let config = TellConfig {
        lan_url: Some("https://rpi-server.lan/jabot/tell".into()),
        ..config()
    };
    let response = super::views::tell_reply(&config, &stored(false));

    assert!(
        content(&response)
            .ends_with("\nOn the LAN, use https://rpi-server.lan/jabot/tell instead.")
    );
}

#[tokio::test]
async fn existing_token_is_shown_without_test_dm() {
    let mut seq = Sequence::new();
    let (responder, reply) = capturing_responder(&mut seq);
    let mut discord = MockDiscordApi::new();
    discord.expect_send_dm().times(0);

    run_tell(
        &tell(repo_returning(stored(false)), config()),
        responder,
        discord,
    )
    .await
    .unwrap();

    let reply = reply.lock().unwrap().clone().unwrap();
    assert!(reply["data"]["content"].as_str().unwrap().contains(TOKEN));
}

#[tokio::test]
async fn closed_dms_get_an_explanation() {
    let mut seq = Sequence::new();
    let (mut responder, _) = capturing_responder(&mut seq);
    responder
        .expect_followup()
        .withf(|followup| followup_content(followup) == text::DMS_CLOSED)
        .times(1)
        .returning(|_| Ok(()));
    let mut discord = MockDiscordApi::new();
    discord
        .expect_send_dm()
        .returning(|_, _| Err(DmError::Closed));

    run_tell(
        &tell(repo_returning(stored(true)), config()),
        responder,
        discord,
    )
    .await
    .unwrap();
}

async fn press_revoke(repo: MockTellRepo, custom_id: &str, expected: &'static str) {
    let mut responder = MockResponder::new();
    responder
        .expect_respond()
        .withf(move |response| {
            let json = json(response);
            // 7 = UpdateMessage: the reply is replaced, and its button removed.
            json["type"] == 7
                && content(response) == expected
                && json["data"]["components"] == serde_json::json!([])
        })
        .times(1)
        .returning(|_| Ok(()));
    let ctx = ctx(responder, MockDiscordApi::new(), any_users());

    tell(repo, config())
        .on_component(&ctx, ComponentRequest::button(user(), custom_id))
        .await
        .unwrap();
}

#[tokio::test]
async fn revoke_deletes_the_token_and_updates_the_reply() {
    let mut repo = MockTellRepo::new();
    repo.expect_revoke()
        .with(eq(user()), eq(TokenId(7)))
        .times(1)
        .returning(|_, _| Ok(true));

    press_revoke(repo, "tell:revoke:7", text::REVOKED).await;
}

#[tokio::test]
async fn stale_revoke_button_says_already_revoked() {
    let mut repo = MockTellRepo::new();
    repo.expect_revoke().returning(|_, _| Ok(false));

    press_revoke(repo, "tell:revoke:7", text::ALREADY_REVOKED).await;
}

#[tokio::test]
async fn malformed_custom_id_is_an_internal_error() {
    let ctx = ctx(MockResponder::new(), MockDiscordApi::new(), any_users());
    let result = tell(MockTellRepo::new(), config())
        .on_component(&ctx, ComponentRequest::button(user(), "tell:bogus"))
        .await;

    assert!(matches!(result, Err(FeatureError::Internal(_))));
}
