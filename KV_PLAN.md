# JABot Plan: `/k` Key-Value Store

**Status:** planned, not started.

## Goals

1. **`/k set <key>`** stores the user's next DM to the bot under `key`. The stored value is the
   message text exactly as typed, Markdown included, so showing it again reproduces the
   formatting.
2. **`/k get [key]`** shows a value, finds keys by substring, or lists every key the user has.
   Lists are paged, 25 keys per page (`PAGE_SIZE`, one constant in `rules.rs`).
3. **Private by default.** Values and lists are ephemeral everywhere except the bot's DM,
   where they're normal messages that stay in the user's DM history.
4. **First message support in the framework.** `/k set` is the first feature that reads
   messages, so this adds the message hook that `REFACTOR_PLAN.md` ("Adding Message Support
   Later") describes.

## Decisions

| Question | Decision | Consequence |
|---|---|---|
| Whose keys | **Per user.** Each user has their own key space | `kv_values` is keyed by `(user_id, name)`; nobody can read another user's values |
| Key format | Trimmed, then lowercased (Unicode-aware, in Rust), 1–64 characters, no `` ` `` or control characters | `ABC ` and `abc` are the same key. Keys can go in inline code and in a 100-character custom id. The slash option sets `min_length(1)`/`max_length(64)`; the handler validates again |
| Value format | The message's `content`, verbatim. At most 2000 characters | That's the most a bot can send in one message. Nitro users can type up to 4000, so a longer message is refused and the bot keeps waiting |
| Attachments, stickers, embeds, forwards | **Not stored** | Discord's attachment URLs expire, so storing them would break. A message with no text is refused and the bot keeps waiting; a message with text and attachments stores the text and says the attachments were dropped |
| Waiting for the value | In memory: one pending set per user, expiring 10 minutes after `/k set` | Lost on restart, which is fine for a 10-minute window. A second `/k set` replaces the first, and the reply says so |
| Timeout | Checked when the next DM arrives | A DM after the deadline gets "timed out, run /k set again" and isn't stored. No timer task, and no message when nobody replies |
| DMs with nothing pending | Ignored | No reply to stray DMs, so later DM features don't fight over them |
| Cancelling | A **Cancel** button on the prompt DM | Otherwise a user who sees the existing value and changes their mind can only wait 10 minutes. The button's custom id carries the pending id, so an old prompt can't cancel a newer `/k set` |
| Deleting | A **Delete** button on the prompt, shown only when the key already exists. It deletes the key at once, with no confirmation step, and ends the pending set | The old value was just sent verbatim in the message above the prompt, so a wrong click can be undone by running `/k set` again and pasting it back. Like Cancel, the button carries the pending id and stops working once the set is used, replaced, cancelled or timed out |
| Autocomplete | On the `key` option of both `/k get` and `/k set`: up to 25 of the user's keys containing what they've typed, prefix matches first | Needs a framework autocomplete hook (see [Autocomplete](#autocomplete)). The suggestions are optional: a new key for `/k set`, or a partial search for `/k get`, can still be typed freely |
| "Redirect" to DMs | An ephemeral reply with a link button to `https://discord.com/channels/@me/<dm channel id>`, plus the prompt DM itself | Discord can't move the user; the DM's notification and the link are the closest thing |
| Lookup rule | Substring match (`instr`) on lowercased keys. An exact match wins; otherwise one match shows its value, several show a list, none is a refusal | See [`/k get`](#k-get) |
| Showing a value | The message content is the value and nothing else, with mentions disabled | The value can be copied or forwarded as-is. The matched key isn't shown (see [Open questions](#open-questions)) |
| Ephemeral or not | Ephemeral unless the interaction's context is `BotDm` | Needs `CommandRequest::context`. Lists follow the same rule as values |
| Paging | Prev/Next buttons with custom id `kv:page:<page>[:<query>]`; the content says "page 2/4, 87 keys". The page size is `rules::PAGE_SIZE` (25) | Each press reloads the keys, so a list never goes stale; the page is clamped to the last one |
| Limit | 1000 keys per user | Bounds storage at about 2 MB per user. Checked at `/k set` for new keys, before anything is sent |
| Gateway intents | `DIRECT_MESSAGES` only | Not privileged, so nothing changes in the Developer Portal. DMs to the bot include their text without `MESSAGE_CONTENT` |
| Names | Command `/k`, feature `kv`, namespace `kv`, tables `kv_*` | `k` alone would be a cryptic namespace in logs and table names |

## Using It

In a server:

```
/k set key:Pasta
  → (ephemeral) "I've DMed you. Send the value for `pasta` there within 10 minutes."  [Open DM]
  → DM: <the current value, verbatim>                     (only if `pasta` exists)
  → DM: "↑ That's the current value of `pasta`. Your next message here replaces it."  [Cancel] [Delete]
                                                          ([Delete] only if `pasta` exists)
  ← user DMs: "**Boil** 12 min, then:\n- salt\n- oil"
  → DM: "Saved `pasta`."

/k get key:pa         → while typing, Discord suggests `pasta`, `paella`, `spam`, ... (autocomplete)
/k get key:pas        → (ephemeral) the value of `pasta`, the only key containing "pas"
/k get key:a          → (ephemeral) "Keys containing `a` — page 1/2, 47 keys" + 25 keys + [◀ Prev] [Next ▶]
/k get                → (ephemeral) every key, paged the same way
```

In the bot's DM, the same commands reply with normal messages, and `/k set` prompts right
there instead of linking.

## Framework Changes

### Interaction context on commands

```rust
// framework/request.rs
pub struct CommandRequest {
    // ...
    /// Where the command was used: a server, the bot's DM, or another private channel.
    pub context: Option<InteractionContext>,
}
```

Filled from `CommandInteraction::context` (present in serenity 0.12.4). `CommandRequest::new`
leaves it `None`; tests set it with a builder method `.context(InteractionContext::BotDm)`.

### `DiscordApi` additions

```rust
/// The DM channel with `user`, creating it if needed. Doesn't check that the user accepts DMs.
async fn dm_channel(&self, user: UserId) -> Result<ChannelId, DiscordError>;
/// Send `message` to `channel`. `DmError::Closed` if it's a DM channel the user has closed.
async fn send_message(&self, channel: ChannelId, message: CreateMessage) -> Result<(), DmError>;
```

- `send_message` takes a `CreateMessage` rather than a string, so a prompt can carry its
  Cancel button and a value can disable mentions. Tests inspect it with `serde_json`, the same
  way they inspect interaction responses.
- `send_dm` stays as it is, so Secret Santa and `tell` don't change.

### Message events

This follows `REFACTOR_PLAN.md`'s outline:

```rust
// framework/feature.rs
/// Gateway intents this feature needs. The bot requests the union over all features.
fn intents(&self) -> GatewayIntents { GatewayIntents::empty() }

async fn on_message(&self, _ctx: &MessageCtx, _msg: MessageRequest) -> Result<(), FeatureError> { Ok(()) }

// framework/request.rs
pub struct MessageRequest {
    pub author: UserId,
    pub channel: ChannelId,
    pub guild: Option<GuildId>,
    pub content: String,
    /// Attachments, stickers and embeds the message carried. Their content isn't kept.
    pub extras: usize,
}

// framework/context.rs
pub struct MessageCtx {
    pub discord: Arc<dyn DiscordApi>,
    pub users: Arc<dyn UserRepo>,
}
```

- **`FeatureRegistry::intents()`** returns the union of every feature's `intents()`.
  `main.rs` passes it to `Client::builder` in place of `GatewayIntents::empty()`.
- **`Bot::message`** calls `registry.dispatch_message(http, msg)`, which:
  1. skips messages from bots, including the bot's own prompts, and system messages (anything
     other than `MessageType::Regular` and `InlineReply`);
  2. calls `on_message` on each feature whose `intents()` include `DIRECT_MESSAGES` or
    `GUILD_MESSAGES`, matching the message's origin;
  3. handles `Err` like interactions do: `User(msg)` is sent to the message's channel with
     `send_message`, and `Internal(e)` is logged and answered with the generic message.
- **No `users.ensure` for messages**, since that's a database write per message. `kv` doesn't
  need it: a pending set always starts with `/k set`, whose interaction already ensured the
  user.
- `MockFeature` gains `intents` and `on_message`. Registry tests: the intents union, bot
  authors skipped, only opted-in features called, and error replies.

### Autocomplete

`FeatureRegistry::dispatch` currently logs and ignores `Interaction::Autocomplete`. Discord
sends one of these as the user types into an option marked `set_autocomplete(true)`, and the
bot answers with up to 25 suggestions.

```rust
// framework/request.rs
pub struct AutocompleteRequest {
    pub user: UserId,
    pub command: String,
    pub subcommand: Option<String>,
    /// The option being typed into, and its text so far.
    pub focused: String,
    pub partial: String,
}

// framework/feature.rs
/// Suggestions for the focused option, as (label, value) pairs. The registry sends at most 25.
async fn on_autocomplete(&self, _ctx: &AutocompleteCtx, _req: AutocompleteRequest)
    -> Result<Vec<(String, String)>, FeatureError> { Ok(vec![]) }

// framework/context.rs
pub struct AutocompleteCtx {
    pub users: Arc<dyn UserRepo>,
}
```

- **Built** from `CommandInteraction::data.autocomplete()`, which finds the focused option
  inside subcommands, plus the same subcommand flattening `CommandRequest` uses.
- **Routed** by command name, like commands.
- **The registry sends the response** (`CreateInteractionResponse::Autocomplete`), truncating
  to 25 choices and to Discord's 100-character label/value limit. Features only return
  data, which keeps their tests free of response builders.
- **No `users.ensure`.** Autocomplete fires on every few keystrokes and writes nothing.
- **Errors can't be shown.** Discord has no way to display an error for autocomplete, so both
  error kinds are logged and answered with an empty list.
- **No responder in the context.** A feature can't reply any other way, and there's nothing to
  DM about.

Registry tests: routing by command name, truncation to 25, and an error answered with an empty
list.

## `kv` Feature

```
src/features/kv/
├── mod.rs          # Kv: impl Feature (commands, intents, on_command/on_component/on_message/on_autocomplete); routing only
├── commands.rs     # /k definition; set and get handlers; key suggestions
├── messages.rs     # DM handler: completes a pending set
├── components.rs   # Prev/Next page buttons, Cancel and Delete buttons
├── custom_id.rs    # KvId: `kv:page:<page>[:<query>]`, `kv:cancel:<pending id>`, `kv:delete:<pending id>`
├── pending.rs      # PendingSets: in-memory pending `/k set`s with expiry
├── model.rs        # Key, Lookup, KvError
├── rules.rs        # pure: normalize_key, resolve, paginate, check_value
├── repo.rs         # trait KvRepo + SqliteKvRepo (kv_* SQL)
├── views.rs, text.rs
└── tests.rs        # handler tests against mocks
```

`Kv { repo: Arc<dyn KvRepo>, pending: PendingSets }`. It's registered unconditionally in
`main.rs`, since it doesn't need the HTTP server.

### Schema (new migration, `migrations/20261005000000_kv.sql`)

```sql
-- kv feature tables (feature-owned, prefixed kv_).

-- One row per stored value. Keys are per user and stored lowercased.
CREATE TABLE kv_values (
    user_id    INTEGER NOT NULL REFERENCES users(id),
    name       TEXT    NOT NULL,  -- the key; `key` is an SQL keyword
    value      TEXT    NOT NULL,  -- message content, verbatim
    updated_at TEXT    NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (user_id, name)
) WITHOUT ROWID;
```

### Repository

```rust
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait KvRepo: Send + Sync {
    async fn get(&self, user: UserId, key: &str) -> Result<Option<String>, sqlx::Error>;
    /// Insert or replace, bumping `updated_at`.
    async fn set(&self, user: UserId, key: &str, value: &str) -> Result<(), sqlx::Error>;
    /// The user's keys containing `filter` (all of them for `None`), sorted.
    async fn keys(&self, user: UserId, filter: Option<&str>) -> Result<Vec<String>, sqlx::Error>;
    async fn count(&self, user: UserId) -> Result<i64, sqlx::Error>;
    /// `Ok(false)` if the key didn't exist.
    async fn delete(&self, user: UserId, key: &str) -> Result<bool, sqlx::Error>;
    /// Up to `limit` keys containing `partial`, those starting with it first, then
    /// alphabetical. For autocomplete.
    async fn suggest(&self, user: UserId, partial: &str, limit: i64) -> Result<Vec<String>, sqlx::Error>;
}
```

- `keys` uses `instr(name, ?) > 0` rather than `LIKE`, so `%` and `_` in a query match
  literally without escaping.
- Loading every matching key and paging in Rust is fine at 1000 keys per user, and keeps the
  paging rule pure and testable.
- `suggest` does its ordering and `LIMIT` in SQL
  (`ORDER BY instr(name, ?) != 1, name LIMIT ?`), because it runs on every few keystrokes.

### Pending sets

```rust
pub struct PendingSets { next_id: AtomicU64, by_user: Mutex<HashMap<UserId, Pending>> }
pub struct Pending { pub id: PendingId, pub key: Key, pub expires_at: Instant }

impl PendingSets {
    /// Start waiting for `user`'s value for `key`. Returns the new id and the key it replaced, if any.
    pub fn start(&self, user: UserId, key: Key, now: Instant) -> (PendingId, Option<Key>);
    /// The user's pending set. An expired one is removed and returned as `Expired`.
    pub fn current(&self, user: UserId, now: Instant) -> Current;   // None | Active(Pending) | Expired(Key)
    /// Remove the pending set if it's still `id`, returning its key. `None` if it was replaced,
    /// used, cancelled or deleted, or has expired.
    pub fn finish(&self, user: UserId, id: PendingId, now: Instant) -> Option<Key>;
}
```

- Like `RateLimiter`, the time is a parameter, so tests drive the clock with plain `Instant`s.
  `on_message`/`on_command` call `Instant::now()` and pass it down.
- `start` drops expired entries when the map holds more than 1024, so abandoned sets can't pile
  up.
- Serenity runs each event in its own task, so two quick DMs can race. Only the one whose
  `finish` succeeds is stored.

### `/k set <key>`

1. `normalize_key`; refuse an invalid key.
2. `repo.get` for the existing value. If the key is new and `repo.count` is at 1000, refuse.
3. `pending.start(user, key, now)`.
4. **In the bot's DM:** respond with the existing value (if any), then follow up with the
   prompt, or respond with the prompt directly when the key is new.
5. **Anywhere else:**
   1. `discord.dm_channel(user)`.
   2. Respond ephemerally with "I've DMed you" and the link button. If step 3 replaced
      another pending key, the reply says so.
   3. `send_message` the existing value (if any), then the prompt.
   4. On `DmError::Closed`: `pending.finish`, then an ephemeral follow-up saying the bot
      can't DM them and how to allow DMs from server members.

Steps 1–5.2 fit in Discord's 3 seconds: one or two indexed queries and one HTTP call before
the response.

The prompt always has **Cancel**, and has **Delete** when the key exists.

### A DM arrives (`on_message`)

1. Ignore it unless `guild` is `None`.
2. `pending.current(author, now)`:
   - `None`: ignore.
   - `Expired(key)`: reply "Your `/k set` for `key` timed out. Run it again."
   - `Active(p)`: continue.
3. `check_value(content, extras)`: refuse no text, or more than 2000 characters, as a
   `FeatureError::User`. The set stays pending, so the user can just send again.
4. `pending.finish(author, p.id, now)`; if it returns `None`, ignore the message (a racing
   DM, Cancel or Delete won).
5. `repo.set`, then reply "Saved `key`.", noting dropped attachments if `extras > 0`.

### `/k get [key]`

- **No key:** list every key, page 1.
- **A key:** normalize it, then `rules::resolve(query, repo.keys(user, Some(query)))`:
  - an exact match → show that value;
  - otherwise exactly one match → show it;
  - otherwise several → list them, page 1;
  - none → "No key contains `query`." (ephemeral refusal).
- No keys at all: "You have no keys yet. Use `/k set`."
- Values and lists are ephemeral unless `context == Some(BotDm)`, and use
  `allowed_mentions` with nothing allowed.

### Buttons

- **Prev/Next** (`kv:page:<page>[:<query>]`): reload `repo.keys` for the presser, clamp the
  page, and `UpdateMessage`. The query may contain `:`, so the id is parsed with
  `splitn(4, ':')`; "no query" (list all) is the three-part form. The keys shown are always the
  presser's own, and the message is ephemeral or in their DM, so nobody else can press it in
  practice anyway. On the first or last page, that button is disabled.
- **Cancel** (`kv:cancel:<pending id>`): `pending.finish(user, id, now)`, then
  `UpdateMessage` to "Cancelled; `key` is unchanged." with the buttons removed.
- **Delete** (`kv:delete:<pending id>`): `pending.finish(user, id, now)`, then
  `repo.delete(user, key)`, then `UpdateMessage` to "Deleted `key`." with the buttons removed.
  If `delete` returns `false` (it was already gone), the message says so.
- When `finish` returns `None` (the set was already used, replaced, cancelled, deleted or timed
  out), both buttons update the prompt to "This prompt has expired. Run `/k set` again." and
  remove the buttons, so an old prompt can never change data.

### Autocomplete (`on_autocomplete`)

- For the `key` option of `get` and `set`: normalize `partial` the same way keys are, without
  the validity checks, and return `repo.suggest(user, partial, 25)` as `(key, key)` pairs.
  An empty `partial` suggests the first 25 keys alphabetically.
- A user with no keys gets an empty list, and Discord shows no suggestions.

## Testing

- **rules.rs**:
  - `normalize_key`: trimming, Unicode lowercasing, and the length, backtick and
    control-character limits.
  - `resolve`: exact match among several, a single partial match, several, none.
  - `paginate`: `PAGE_SIZE` per page, page count, clamping past the end, an empty list.
  - `check_value`: empty, whitespace-only, exactly 2000 characters, 2001, and attachments with
    text.
- **pending.rs** (fake `Instant`s): `start` replaces and reports the old key; expiry at 10
  minutes; `finish` with a stale id and after expiry; pruning drops only expired entries.
- **custom_id.rs**: round trips for page, cancel and delete; a query containing `:`; list-all
  ids; malformed ids.
- **repo.rs** (`#[sqlx::test]`):
  - Upsert replaces and bumps `updated_at`.
  - Users don't see each other's keys.
  - `keys` matches substrings with `%` and `_` treated literally.
  - `count`.
  - `delete` reports whether the key existed.
  - `suggest` puts prefix matches first and respects the limit.
- **tests.rs** (`MockKvRepo`, `MockDiscordApi`, `MockResponder`):
  - `/k set` in a server: ephemeral link reply, then existing value and prompt in the DM;
    a new key sends only the prompt.
  - `/k set` in the bot's DM: non-ephemeral, no `dm_channel` call.
  - `/k set` with closed DMs: follow-up, and the set is no longer pending.
  - `/k set` at 1000 keys: refused for a new key, allowed for an existing one.
  - A DM with a pending set stores the content verbatim and confirms; a second DM is ignored.
  - A DM after 10 minutes reports the timeout and stores nothing; one with nothing pending gets
    no reply.
  - Too long or textless DMs are refused, and a valid DM after that is still stored.
  - `/k get`: each `resolve` outcome, ephemeral in a server and not in the bot's DM, mentions
    disabled.
  - Paging buttons update the message and disable Prev/Next at the ends.
  - The prompt has Delete only for an existing key.
  - Cancel and Delete with a current id; with a stale id, neither touches the repo.
  - Delete removes the key and ends the set, so a DM after it is ignored.
  - Autocomplete returns `suggest`'s keys for both subcommands, lowercasing the partial input.
- **framework**: the registry tests listed under [Message events](#message-events) and
  [Autocomplete](#autocomplete), and `CommandRequest` conversion picking up `context`.
- **Manual**, against a test server:
  - The full `/k set` flow from a server and from the bot's DM, with Markdown (bold, lists,
    code blocks, spoilers) that should look the same when shown with `/k get`.
  - Overwriting an existing key, Cancel, Delete, and a timeout (temporarily shorten it).
  - Autocomplete in a server and in the bot's DM, with no keys and with more than 25 keys.
  - Paging through 55 keys.
  - With "Allow DMs from server members" off.

## Steps

Each step compiles, passes `cargo test` and `cargo clippy --all-targets`, and gets its own
commit.

1. **Command context.** Add `CommandRequest::context`.
2. **`DiscordApi` additions.** Add `dm_channel` and `send_message`, with the serenity impls.
3. **Message events.** Add `Feature::intents`/`on_message`, `MessageRequest`, `MessageCtx`,
   `FeatureRegistry::intents`/`dispatch_message`, `Bot::message`, and pass the intents in
   `main.rs`. With no feature opting in, the bot still requests no intents.
4. **Autocomplete.** Add `AutocompleteRequest`, `AutocompleteCtx`, `Feature::on_autocomplete`,
   and the registry routing and response.
5. **`kv` storage.** Add the migration, run `sqlx migrate run`, add `KvRepo` and
   `SqliteKvRepo` with their tests, then `cargo sqlx prepare -- --all-targets` and commit
   `.sqlx/`.
6. **Rules, pending sets and custom ids.** Pure code and its tests.
7. **`/k get`, paging and autocomplete.** Register `Kv` in `main.rs`, with only `get` in the
   command.
8. **`/k set`.** The subcommand, the DM handler, Cancel and Delete, and `intents()` returning
   `DIRECT_MESSAGES`.
9. **Docs.**
   - CLAUDE.md: the architecture tree (`features/kv/`, `MessageCtx`, `MessageRequest`,
     `AutocompleteRequest`); the conventions line "The bot requests no gateway intents"
     becomes "each feature declares its intents; the bot requests their union"; replace
     "Message events aren't supported yet" with a note on `on_message` and `on_autocomplete`.
   - README: how to use `/k`.
10. **Manual test, then deploy** with `./deploy.sh`. The migration applies at startup.

## Open Questions

Each has a default in this plan; say if you want it the other way.

- **Show the matched key with a value?** The default sends the value alone, so it reproduces
  exactly. When `/k get pas` resolves to `pasta`, the user isn't told which key matched. The
  alternative is a header line, which costs space within the 2000-character limit.
- **Attachments.** Storing them properly means downloading and re-uploading the files, which
  needs blob storage and size limits. Deferred.
- **The 1000-key limit** wasn't in the request; it's small and easy to drop.

## Deferred

- **`/k delete <key>`** and **`/k rename`**. Today a key can only be deleted from a `/k set`
  prompt.
- **A select menu on list pages** to open a key directly. A page of 25 keys fits a select
  menu's 25 options exactly, so `PAGE_SIZE` must stay at 25 or less if this is added.
- **Editing the prompt DM after the value is saved**, to remove its Cancel and Delete buttons. Needs the
  prompt's message id and a `DiscordApi::edit_message`.
- **Pending sets that survive restarts**, if 10-minute windows lost to deploys turn out to
  matter.
