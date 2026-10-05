//! Handler tests: the real handlers against mocked storage and Discord.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use mockall::predicate::eq;
use serde_json::{Value, json};
use serenity::all::{ChannelId, CommandDataOptionValue, InteractionContext, UserId};

use super::messages;
use super::model::PendingId;
use super::pending::{Current, TIMEOUT};
use super::repo::MockKvRepo;
use super::rules::{MAX_KEYS, PAGE_SIZE, normalize_key};
use super::{Kv, text};
use crate::framework::testing::{any_users, ctx, message_ctx};
use crate::framework::{
    AutocompleteCtx, AutocompleteRequest, CommandRequest, ComponentRequest, DmError, Feature,
    FeatureError, MessageRequest, MockDiscordApi, MockResponder, MockUserRepo,
};

const DM_CHANNEL: u64 = 9;

fn user() -> UserId {
    UserId::new(1)
}

fn kv(repo: MockKvRepo) -> Kv {
    Kv::new(Arc::new(repo))
}

/// Everything the handlers sent, in order: `("respond" | "followup" | "send", json)`.
#[derive(Clone, Default)]
struct Log(Arc<Mutex<Vec<(&'static str, Value)>>>);

impl Log {
    fn push(&self, kind: &'static str, value: Value) {
        self.0.lock().unwrap().push((kind, value));
    }

    fn kinds(&self) -> Vec<&'static str> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .map(|(kind, _)| *kind)
            .collect()
    }

    fn get(&self, index: usize) -> Value {
        self.0.lock().unwrap()[index].1.clone()
    }
}

fn responder(log: &Log) -> MockResponder {
    let mut responder = MockResponder::new();
    let respond_log = log.clone();
    responder.expect_respond().returning(move |response| {
        respond_log.push("respond", serde_json::to_value(&response).unwrap());
        Ok(())
    });
    let followup_log = log.clone();
    responder.expect_followup().returning(move |followup| {
        followup_log.push("followup", serde_json::to_value(&followup).unwrap());
        Ok(())
    });
    responder
}

/// A Discord whose DMs to the user go to `DM_CHANNEL` and succeed.
fn discord(log: &Log) -> MockDiscordApi {
    discord_with(log, || Ok(()))
}

fn discord_with(log: &Log, result: fn() -> Result<(), DmError>) -> MockDiscordApi {
    let mut discord = MockDiscordApi::new();
    discord
        .expect_dm_channel()
        .with(eq(user()))
        .returning(|_| Ok(ChannelId::new(DM_CHANNEL)));
    let log = log.clone();
    discord
        .expect_send_message()
        .returning(move |channel, message| {
            assert_eq!(channel, ChannelId::new(DM_CHANNEL));
            log.push("send", serde_json::to_value(&message).unwrap());
            result()
        });
    discord
}

fn set_req(key: &str) -> CommandRequest {
    CommandRequest::new(user(), text::CMD_K)
        .subcommand(text::SUB_SET)
        .option(text::OPT_KEY, CommandDataOptionValue::String(key.into()))
}

fn get_req(key: Option<&str>) -> CommandRequest {
    let req = CommandRequest::new(user(), text::CMD_K).subcommand(text::SUB_GET);
    match key {
        Some(key) => req.option(text::OPT_KEY, CommandDataOptionValue::String(key.into())),
        None => req,
    }
}

fn dm(content: &str) -> MessageRequest {
    MessageRequest::dm(user(), ChannelId::new(DM_CHANNEL), content)
}

async fn command(feature: &Kv, log: &Log, discord: MockDiscordApi, req: CommandRequest) {
    let ctx = ctx(responder(log), discord, any_users());
    feature.on_command(&ctx, req).await.unwrap();
}

async fn command_err(feature: &Kv, req: CommandRequest) -> String {
    let ctx = ctx(MockResponder::new(), MockDiscordApi::new(), any_users());
    match feature.on_command(&ctx, req).await {
        Err(FeatureError::User(message)) => message,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

async fn press(feature: &Kv, log: &Log, custom_id: String) {
    let ctx = ctx(responder(log), MockDiscordApi::new(), any_users());
    feature
        .on_component(&ctx, ComponentRequest::button(user(), custom_id))
        .await
        .unwrap();
}

async fn receive(
    feature: &Kv,
    log: &Log,
    msg: MessageRequest,
    now: Instant,
) -> Result<(), FeatureError> {
    let ctx = message_ctx(discord(log));
    Ok(messages::receive(feature, &ctx, &msg, now).await?)
}

/// The pending id of the user's waiting `/k set`.
fn pending_id(feature: &Kv) -> PendingId {
    match feature.pending.current(user(), Instant::now()) {
        Current::Active(pending) => pending.id,
        other => panic!("nothing waiting: {other:?}"),
    }
}

fn button_ids(message: &Value) -> Vec<String> {
    message["components"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|row| row["components"].as_array().unwrap())
        .map(|button| {
            button["custom_id"]
                .as_str()
                .or(button["url"].as_str())
                .unwrap()
                .to_string()
        })
        .collect()
}

fn ephemeral(data: &Value) -> bool {
    data["flags"].as_u64().is_some_and(|flags| flags & 64 != 0)
}

fn repo_with_value(key: &'static str, value: Option<&'static str>) -> MockKvRepo {
    let mut repo = MockKvRepo::new();
    repo.expect_get()
        .withf(move |u, k| *u == user() && k == key)
        .returning(move |_, _| Ok(value.map(str::to_string)));
    repo
}

// /k set

#[tokio::test]
async fn set_in_a_server_links_to_the_dm_then_sends_value_and_prompt() {
    let mut repo = repo_with_value("pasta", Some("**old** <@2>"));
    repo.expect_count().times(0);
    let feature = kv(repo);
    let log = Log::default();

    command(&feature, &log, discord(&log), set_req("  Pasta ")).await;

    assert_eq!(log.kinds(), ["respond", "send", "send"]);
    let reply = log.get(0)["data"].clone();
    assert!(ephemeral(&reply));
    assert!(reply["content"].as_str().unwrap().contains("`pasta`"));
    assert_eq!(
        button_ids(&reply),
        [format!("https://discord.com/channels/@me/{DM_CHANNEL}")]
    );
    let value = log.get(1);
    assert_eq!(value["content"], "**old** <@2>");
    assert_eq!(value["allowed_mentions"]["parse"], json!([]));
    let id = pending_id(&feature);
    assert_eq!(
        button_ids(&log.get(2)),
        [format!("kv:cancel:{id}"), format!("kv:delete:{id}")]
    );
}

#[tokio::test]
async fn set_for_a_new_key_sends_only_the_prompt_with_cancel() {
    let mut repo = repo_with_value("pasta", None);
    repo.expect_count().returning(|_| Ok(3));
    let feature = kv(repo);
    let log = Log::default();

    command(&feature, &log, discord(&log), set_req("pasta")).await;

    assert_eq!(log.kinds(), ["respond", "send"]);
    let prompt = log.get(1);
    assert!(
        prompt["content"]
            .as_str()
            .unwrap()
            .starts_with("Send the value for `pasta`")
    );
    assert_eq!(
        button_ids(&prompt),
        [format!("kv:cancel:{}", pending_id(&feature))]
    );
}

#[tokio::test]
async fn set_in_the_bots_dm_replies_in_place() {
    let feature = kv(repo_with_value("pasta", Some("old")));
    let log = Log::default();
    let mut discord = MockDiscordApi::new();
    discord.expect_dm_channel().times(0);
    discord.expect_send_message().times(0);

    let req = set_req("pasta").context(InteractionContext::BotDm);
    command(&feature, &log, discord, req).await;

    assert_eq!(log.kinds(), ["respond", "followup"]);
    let value = log.get(0)["data"].clone();
    assert_eq!(value["content"], "old");
    assert!(!ephemeral(&value));
    let prompt = log.get(1);
    assert!(!ephemeral(&prompt));
    assert_eq!(button_ids(&prompt).len(), 2);
}

#[tokio::test]
async fn set_for_a_new_key_in_the_bots_dm_prompts_directly() {
    let mut repo = repo_with_value("pasta", None);
    repo.expect_count().returning(|_| Ok(0));
    let feature = kv(repo);
    let log = Log::default();

    let req = set_req("pasta").context(InteractionContext::BotDm);
    command(&feature, &log, MockDiscordApi::new(), req).await;

    assert_eq!(log.kinds(), ["respond"]);
    let prompt = log.get(0)["data"].clone();
    assert!(!ephemeral(&prompt));
    assert_eq!(button_ids(&prompt).len(), 1);
}

#[tokio::test]
async fn second_set_says_what_it_replaced() {
    let mut repo = MockKvRepo::new();
    repo.expect_get().returning(|_, _| Ok(None));
    repo.expect_count().returning(|_| Ok(0));
    let feature = kv(repo);
    let log = Log::default();

    command(&feature, &log, discord(&log), set_req("pasta")).await;
    command(&feature, &log, discord(&log), set_req("pizza")).await;

    let reply = log.get(2)["data"]["content"].as_str().unwrap().to_string();
    assert!(reply.contains("`pizza`") && reply.contains("unfinished `/k set` for `pasta`"));
}

#[tokio::test]
async fn set_with_closed_dms_follows_up_and_stops_waiting() {
    let mut repo = repo_with_value("pasta", None);
    repo.expect_count().returning(|_| Ok(0));
    let feature = kv(repo);
    let log = Log::default();

    let discord = discord_with(&log, || Err(DmError::Closed));
    command(&feature, &log, discord, set_req("pasta")).await;

    assert_eq!(log.kinds(), ["respond", "send", "followup"]);
    assert_eq!(log.get(2)["content"], text::DMS_CLOSED);
    assert_eq!(
        feature.pending.current(user(), Instant::now()),
        Current::Nothing
    );
}

#[tokio::test]
async fn set_refuses_a_new_key_at_the_limit() {
    let mut repo = repo_with_value("pasta", None);
    repo.expect_count().returning(|_| Ok(MAX_KEYS));
    let feature = kv(repo);

    let refusal = command_err(&feature, set_req("pasta")).await;

    assert!(refusal.starts_with(&format!("You already have {MAX_KEYS} keys")));
    assert_eq!(
        feature.pending.current(user(), Instant::now()),
        Current::Nothing
    );
}

#[tokio::test]
async fn set_allows_replacing_a_key_at_the_limit() {
    let mut repo = repo_with_value("pasta", Some("old"));
    repo.expect_count().times(0);
    let feature = kv(repo);
    let log = Log::default();

    command(&feature, &log, discord(&log), set_req("pasta")).await;

    assert!(matches!(
        feature.pending.current(user(), Instant::now()),
        Current::Active(_)
    ));
}

#[tokio::test]
async fn set_refuses_an_invalid_key() {
    let feature = kv(MockKvRepo::new());

    assert_eq!(
        command_err(&feature, set_req("a`b")).await,
        text::INVALID_KEY
    );
}

// DMs

/// A feature waiting for the value of `pasta`, with `repo` behind it.
fn waiting(repo: MockKvRepo, now: Instant) -> Kv {
    let feature = kv(repo);
    feature
        .pending
        .start(user(), normalize_key("pasta").unwrap(), now);
    feature
}

fn stores(content: &'static str) -> MockKvRepo {
    let mut repo = MockKvRepo::new();
    repo.expect_set()
        .withf(move |u, k, v| *u == user() && k == "pasta" && v == content)
        .times(1)
        .returning(|_, _, _| Ok(()));
    repo
}

#[tokio::test]
async fn dm_stores_the_content_verbatim_once() {
    let now = Instant::now();
    let content = "**Boil** 12 min, then:\n- salt\n- `oil`\n||spoiler||";
    let feature = waiting(stores(content), now);
    let log = Log::default();

    receive(&feature, &log, dm(content), now).await.unwrap();
    receive(&feature, &log, dm("again"), now).await.unwrap();

    assert_eq!(log.kinds(), ["send"]);
    assert_eq!(log.get(0)["content"], "Saved `pasta`.");
}

#[tokio::test]
async fn dm_mentions_dropped_attachments() {
    let now = Instant::now();
    let feature = waiting(stores("text"), now);
    let log = Log::default();

    let mut msg = dm("text");
    msg.extras = 2;
    receive(&feature, &log, msg, now).await.unwrap();

    let reply = log.get(0)["content"].as_str().unwrap().to_string();
    assert!(reply.contains("Attachments and stickers aren't stored"));
}

#[tokio::test]
async fn dm_after_the_timeout_is_reported_once_and_not_stored() {
    let now = Instant::now();
    let mut repo = MockKvRepo::new();
    repo.expect_set().times(0);
    let feature = waiting(repo, now);
    let log = Log::default();

    receive(&feature, &log, dm("late"), now + TIMEOUT)
        .await
        .unwrap();
    receive(&feature, &log, dm("later"), now + TIMEOUT)
        .await
        .unwrap();

    assert_eq!(log.kinds(), ["send"]);
    assert!(
        log.get(0)["content"]
            .as_str()
            .unwrap()
            .contains("timed out")
    );
}

#[tokio::test]
async fn dm_with_nothing_waiting_is_ignored() {
    let feature = kv(MockKvRepo::new());
    let log = Log::default();

    receive(&feature, &log, dm("hello"), Instant::now())
        .await
        .unwrap();

    assert!(log.kinds().is_empty());
}

#[tokio::test]
async fn messages_in_servers_are_ignored() {
    let now = Instant::now();
    let mut repo = MockKvRepo::new();
    repo.expect_set().times(0);
    let feature = waiting(repo, now);
    let log = Log::default();

    let mut msg = dm("hello");
    msg.guild = Some(serenity::all::GuildId::new(3));
    receive(&feature, &log, msg, now).await.unwrap();

    assert!(log.kinds().is_empty());
}

#[tokio::test]
async fn refused_dms_keep_the_set_waiting() {
    let now = Instant::now();
    let feature = waiting(stores("short"), now);
    let log = Log::default();

    for bad in ["".to_string(), "a".repeat(2001)] {
        let result = receive(&feature, &log, dm(&bad), now).await;
        assert!(matches!(result, Err(FeatureError::User(_))), "{bad:?}");
    }
    receive(&feature, &log, dm("short"), now).await.unwrap();

    assert_eq!(log.kinds(), ["send"]);
}

// Cancel and Delete

#[tokio::test]
async fn cancel_ends_the_set_without_storing() {
    let now = Instant::now();
    let mut repo = MockKvRepo::new();
    repo.expect_set().times(0);
    repo.expect_delete().times(0);
    let feature = waiting(repo, now);
    let log = Log::default();
    let id = pending_id(&feature);

    press(&feature, &log, format!("kv:cancel:{id}")).await;
    receive(&feature, &log, dm("value"), now).await.unwrap();

    assert_eq!(log.kinds(), ["respond"]);
    let update = log.get(0);
    assert_eq!(update["type"], 7);
    assert_eq!(
        update["data"]["content"],
        "Cancelled; `pasta` is unchanged."
    );
    assert_eq!(update["data"]["components"], json!([]));
}

#[tokio::test]
async fn delete_removes_the_key_and_ends_the_set() {
    let now = Instant::now();
    let mut repo = MockKvRepo::new();
    repo.expect_delete()
        .withf(|u, k| *u == user() && k == "pasta")
        .times(1)
        .returning(|_, _| Ok(true));
    repo.expect_set().times(0);
    let feature = waiting(repo, now);
    let log = Log::default();
    let id = pending_id(&feature);

    press(&feature, &log, format!("kv:delete:{id}")).await;
    receive(&feature, &log, dm("value"), now).await.unwrap();

    assert_eq!(log.kinds(), ["respond"]);
    assert_eq!(log.get(0)["data"]["content"], "Deleted `pasta`.");
    assert_eq!(log.get(0)["data"]["components"], json!([]));
}

#[tokio::test]
async fn stale_prompt_buttons_change_nothing() {
    let now = Instant::now();
    let mut repo = MockKvRepo::new();
    repo.expect_delete().times(0);
    let feature = waiting(repo, now);
    let log = Log::default();
    let stale = PendingId(pending_id(&feature).0 - 1);

    press(&feature, &log, format!("kv:delete:{stale}")).await;
    press(&feature, &log, format!("kv:cancel:{stale}")).await;

    for i in 0..2 {
        assert_eq!(log.get(i)["data"]["content"], text::PROMPT_EXPIRED);
        assert_eq!(log.get(i)["data"]["components"], json!([]));
    }
    assert!(matches!(
        feature.pending.current(user(), now),
        Current::Active(_)
    ));
}

// /k get

fn keys(n: usize, prefix: &str) -> Vec<String> {
    (0..n).map(|i| format!("{prefix}{i:03}")).collect()
}

fn repo_with_matches(filter: Option<&'static str>, matches: Vec<String>) -> MockKvRepo {
    let mut repo = MockKvRepo::new();
    repo.expect_keys()
        .withf(move |u, f| *u == user() && *f == filter)
        .returning(move |_, _| Ok(matches.clone()));
    repo
}

async fn get(feature: &Kv, req: CommandRequest) -> Value {
    let log = Log::default();
    command(feature, &log, MockDiscordApi::new(), req).await;
    assert_eq!(log.kinds(), ["respond"]);
    log.get(0)["data"].clone()
}

#[tokio::test]
async fn get_shows_an_exact_match_ephemerally_without_mentions() {
    let mut repo = repo_with_matches(Some("pasta"), vec!["pasta".into(), "pasta sauce".into()]);
    repo.expect_get()
        .withf(|_, k| k == "pasta")
        .returning(|_, _| Ok(Some("@everyone **boil**".into())));
    let feature = kv(repo);

    let reply = get(&feature, get_req(Some("Pasta"))).await;

    assert_eq!(reply["content"], "@everyone **boil**");
    assert!(ephemeral(&reply));
    assert_eq!(reply["allowed_mentions"]["parse"], json!([]));
}

#[tokio::test]
async fn get_in_the_bots_dm_is_not_ephemeral() {
    let mut repo = repo_with_matches(Some("pasta"), vec!["pasta".into()]);
    repo.expect_get().returning(|_, _| Ok(Some("boil".into())));
    let feature = kv(repo);

    let reply = get(
        &feature,
        get_req(Some("pasta")).context(InteractionContext::BotDm),
    )
    .await;

    assert!(!ephemeral(&reply));
}

#[tokio::test]
async fn get_shows_the_only_partial_match() {
    let mut repo = repo_with_matches(Some("pas"), vec!["pasta".into()]);
    repo.expect_get()
        .withf(|_, k| k == "pasta")
        .returning(|_, _| Ok(Some("boil".into())));
    let feature = kv(repo);

    assert_eq!(get(&feature, get_req(Some("pas"))).await["content"], "boil");
}

#[tokio::test]
async fn get_lists_several_matches_in_pages() {
    let feature = kv(repo_with_matches(Some("k"), keys(PAGE_SIZE + 5, "k")));

    let reply = get(&feature, get_req(Some("k"))).await;

    let content = reply["content"].as_str().unwrap();
    assert!(content.starts_with(&format!(
        "Keys containing `k` — page 1/2, {} keys",
        PAGE_SIZE + 5
    )));
    assert_eq!(content.lines().count(), 1 + PAGE_SIZE);
    assert!(ephemeral(&reply));
    assert_eq!(button_ids(&reply), ["kv:page:0:k", "kv:page:1:k"]);
    let prev = &reply["components"][0]["components"][0];
    assert_eq!(prev["disabled"], true);
}

#[tokio::test]
async fn get_with_no_match_is_refused() {
    let feature = kv(repo_with_matches(Some("x"), vec![]));

    assert_eq!(
        command_err(&feature, get_req(Some("x"))).await,
        "No key contains `x`."
    );
}

#[tokio::test]
async fn get_without_a_key_lists_all_keys() {
    let feature = kv(repo_with_matches(None, vec!["pasta".into()]));

    let reply = get(&feature, get_req(None)).await;

    assert_eq!(reply["content"], "Your keys — page 1/1, 1 key\n`pasta`");
    assert!(button_ids(&reply).is_empty());
}

#[tokio::test]
async fn get_without_any_keys_says_so() {
    let feature = kv(repo_with_matches(None, vec![]));

    assert_eq!(get(&feature, get_req(None)).await["content"], text::NO_KEYS);
}

#[tokio::test]
async fn page_buttons_reload_and_update_the_list() {
    let all = keys(PAGE_SIZE + 5, "k");
    let feature = kv(repo_with_matches(Some("k"), all.clone()));
    let log = Log::default();

    press(&feature, &log, "kv:page:1:k".into()).await;

    let update = log.get(0);
    assert_eq!(update["type"], 7);
    let content = update["data"]["content"].as_str().unwrap();
    assert!(content.starts_with("Keys containing `k` — page 2/2"));
    assert!(content.ends_with(&format!("`{}`", all.last().unwrap())));
    let buttons = &update["data"]["components"][0]["components"];
    assert_eq!(buttons[0]["custom_id"], "kv:page:0:k");
    assert_eq!(buttons[0]["disabled"], false);
    assert_eq!(buttons[1]["disabled"], true);
}

#[tokio::test]
async fn page_button_after_every_key_is_gone() {
    let feature = kv(repo_with_matches(None, vec![]));
    let log = Log::default();

    press(&feature, &log, "kv:page:3".into()).await;

    assert_eq!(log.get(0)["data"]["content"], text::NO_KEYS);
    assert_eq!(log.get(0)["data"]["components"], json!([]));
}

// Autocomplete

fn autocomplete_ctx() -> AutocompleteCtx {
    AutocompleteCtx {
        users: Arc::new(MockUserRepo::new()),
    }
}

#[tokio::test]
async fn autocomplete_suggests_the_users_keys() {
    let mut repo = MockKvRepo::new();
    repo.expect_suggest()
        .with(eq(user()), eq("pa"), eq(25))
        .times(2)
        .returning(|_, _, _| Ok(vec!["pasta".into(), "spam".into()]));
    let feature = kv(repo);

    for subcommand in [text::SUB_GET, text::SUB_SET] {
        let req =
            AutocompleteRequest::new(user(), text::CMD_K, Some(subcommand), text::OPT_KEY, " PA");
        let suggestions = feature
            .on_autocomplete(&autocomplete_ctx(), req)
            .await
            .unwrap();
        assert_eq!(
            suggestions,
            [
                ("pasta".to_string(), "pasta".to_string()),
                ("spam".to_string(), "spam".to_string())
            ]
        );
    }
}

#[tokio::test]
async fn autocomplete_ignores_other_options() {
    let mut repo = MockKvRepo::new();
    repo.expect_suggest().times(0);
    let feature = kv(repo);

    let req = AutocompleteRequest::new(user(), text::CMD_K, Some(text::SUB_GET), "other", "pa");
    let suggestions = feature
        .on_autocomplete(&autocomplete_ctx(), req)
        .await
        .unwrap();
    assert!(suggestions.is_empty());
}

// Definition

#[test]
fn command_has_set_and_get_with_autocompleted_keys() {
    let command = serde_json::to_value(super::commands::k_command()).unwrap();

    assert_eq!(command["name"], "k");
    let subcommands = command["options"].as_array().unwrap();
    let names: Vec<_> = subcommands.iter().map(|s| s["name"].clone()).collect();
    assert_eq!(names, [json!("set"), json!("get")]);
    for (subcommand, required) in subcommands.iter().zip([true, false]) {
        let key = &subcommand["options"][0];
        assert_eq!(key["name"], "key");
        assert_eq!(key["autocomplete"], true);
        assert_eq!(key["required"], required);
        assert_eq!(key["max_length"], 64);
    }
}
