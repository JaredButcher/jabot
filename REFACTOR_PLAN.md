# JABot Refactor Plan: Feature Modules

## Goals

1. Reduce `main.rs` to bot wiring: config, database pool, Discord client, and the `Bot`
   `EventHandler`. It should not know about any specific feature.
2. Move each capability (Secret Santa now, more later) into its own **feature module**. The
   module owns its slash commands, component handlers, modal handlers, SQL, and text.
3. A feature registers with the bot in one line. Adding a feature later should not require
   editing any existing feature or the router.
4. A feature receives its dependencies (database, Discord, shared users) as **traits**, so every
   handler can be unit tested with [`mockall`](https://docs.rs/mockall) without a live Discord
   connection or a database.
5. Fix the bugs found during review. Each fix gets a regression test and its own commit (see
   [Bug Fixes](#bug-fixes)).

## Decisions

| Question | Decision | Consequence |
|---|---|---|
| Existing data | **None.** The bot isn't deployed and the database is empty | Rewrite the initial migration in place; no data migration, backfill or repair needed |
| Table naming | Prefix Secret Santa tables with `ss_` | Done in the rewritten initial migration (see [Schema](#schema)) |
| Users table | **Shared** `users` table owned by the framework | `framework::users::UserRepo` trait; Secret Santa keeps only SS-specific per-user data (`global_wish`) in `ss_users` |
| Message hook | **Drop** the message hook and all gateway intents | No `on_message`; the bot requests `GatewayIntents::empty()`. Interactions don't need intents, so nothing restricted is requested. Add message support when a feature needs it (see [Adding message support later](#adding-message-support-later)) |
| Join flow | **Auto-join**: users the host adds are participants immediately | Remove `/ss join`, the unused Leave button and its handler, and the `joined` column |

## The Idea in One Picture

```
                        ┌──────────────────────── main.rs ────────────────────────┐
                        │  Config · SqlitePool · migrations · serenity Client       │
                        │                                                           │
 Discord gateway ─────▶ │  Bot (impl EventHandler)                                  │
                        │    ready()            → registry.commands()  → Discord    │
                        │    interaction_create → registry.dispatch(interaction)    │
                        └───────────────────────────┬───────────────────────────────┘
                                                    │
                        ┌──────────── framework/ ───▼───────────────────────────────┐
                        │  FeatureRegistry: routes by command name / custom-id ns   │
                        │  serenity types → CommandRequest / ComponentRequest /     │
                        │                   ModalRequest                            │
                        │  InteractionCtx { responder, discord, users }             │
                        │  traits: Feature, Responder, DiscordApi, UserRepo         │
                        │  shared `users` table · central error → reply + log       │
                        └───────────────────────────┬───────────────────────────────┘
                                                    │ &dyn Feature
                 ┌──────────────────────────────────┼─────────────────────────┐
                 ▼                                  ▼                         ▼
     features/secret_santa/              features/<next>/            features/<later>/
       impl Feature                        impl Feature                ...
       repo: Arc<dyn SecretSantaRepo>      repo: Arc<dyn NextRepo>
```

## Three Design Rules

### 1. Mock the feature's repository, not "SQL"

`sqlx::SqlitePool` and `sqlx::query!` can't be mocked, and a generic "run this SQL string"
trait would give meaningless mocks. Instead, each feature defines **its own repository trait**
with methods named after what the feature needs (`get_event`, `start_event`, and so on). Its
sqlx implementation lives inside the feature folder. `main.rs` builds it from the shared pool:

```rust
SecretSanta::new(Arc::new(SqliteSecretSantaRepo::new(pool.clone())))
```

Shared, cross-feature data (the `users` table) follows the same pattern, with the trait
defined in `framework/`.

### 2. Split "Discord" into two traits

Discord has two kinds of dependency with different lifetimes:

| Trait | Scope | Methods | Why separate |
|---|---|---|---|
| `Responder` | one interaction | `respond(CreateInteractionResponse)`, `followup(..)`, `has_responded()` | Needs the interaction's id and token, which only exist per event |
| `DiscordApi` | whole bot | `send_dm(user, text)`, `user_name(user)` | Needs only `Arc<Http>` |

The framework builds both for each interaction and passes them in `InteractionCtx`. The serenity
`Context` never reaches feature code.

### 3. Pass plain request structs, not serenity types

Serenity's `CommandInteraction`, `ComponentInteraction` and `ModalInteraction` are
`#[non_exhaustive]` and very hard to build in a test. The framework converts them into small
data structs, so a test can write `ComponentRequest::button(user, "ss:btn:start:5")`.

Subcommand and option flattening happens once, in this conversion. Getting it wrong in each
handler is what broke `/ss join`.

On the output side, features keep building serenity's `CreateInteractionResponse`. Those
builders are plain data, implement `Serialize`, and are easy to check in tests with
`serde_json::to_value`.

## Proposed Layout

```
src/
├── main.rs                        # Config/token, SqlitePool, migrate!, Bot + EventHandler, feature registration
├── lib.rs                         # pub mod framework; pub mod features;  (lets tests/ reach them)
│
├── framework/                     # shared by all features; knows nothing about Secret Santa
│   ├── mod.rs                     # re-exports
│   ├── feature.rs                 # trait Feature
│   ├── request.rs                 # CommandRequest, ComponentRequest, ModalRequest, Options
│   ├── context.rs                 # InteractionCtx
│   ├── discord.rs                 # traits Responder + DiscordApi, and Serenity* impls
│   ├── users.rs                   # trait UserRepo (+ automock) and SqliteUserRepo — owns the `users` table
│   ├── registry.rs                # FeatureRegistry: register, validate, route, commands()
│   └── error.rs                   # FeatureError (User / Internal)
│
└── features/
    ├── mod.rs                     # pub mod secret_santa;
    └── secret_santa/
        ├── mod.rs                 # pub struct SecretSanta; impl Feature (routing only)
        ├── commands.rs            # CreateCommand for /ss + handlers: create, info, list
        ├── components.rs          # participant select, start, end, cancel handlers
        ├── modals.rs              # create/edit modal submission handlers
        ├── custom_id.rs           # enum SsId { CreateModal, EditModal(EventId), UserSelect(EventId), Start(EventId), End(EventId), Cancel(EventId) }
        ├── model.rs               # EventId, EventStatus, Event, Participant, SsError
        ├── rules.rs               # pure: ensure_host, transitions, assign_santas, participant_diff
        ├── repo.rs                # trait SecretSantaRepo (+ automock) and SqliteSecretSantaRepo — owns ss_* tables
        ├── views.rs               # build CreateInteractionResponse / CreateModal / buttons from model
        ├── text.rs                # user-facing strings (was strings.rs)
        └── tests.rs               # handler tests using the mocks (#[cfg(test)])
```

- `main.rs` still owns the `Bot` struct, the SQL connection and the Discord client.
- Routing and conversion live in `framework/`, so features can import the shared traits and
  the router can be unit tested on its own.
- Secret Santa gets a folder because it's about 800 lines of handler code even after the
  trimming. A small future feature can be a single file, such as `features/ping.rs`.

## Schema

There is no deployed data, so the existing initial migration is **rewritten in place** rather
than adding a second migration.

Compared with today's schema, the rewrite:

- Adds a framework-owned `users` table.
- Renames the Secret Santa tables with an `ss_` prefix.
- Drops the `joined` column (auto-join).
- Orders the participants primary key as `(event_id, user_id)`, which matches how it is
  queried.
- Adds `ON DELETE CASCADE`, so removing an event cleans up its participants.

```sql
-- migrations/20250916010101_initial_migration.sql  (replaces current contents)

-- Framework-owned: one row per Discord user the bot has seen.
CREATE TABLE users (
    id         INTEGER PRIMARY KEY NOT NULL,              -- Discord user id
    created_at TEXT    NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Secret Santa-specific per-user data.
CREATE TABLE ss_users (
    user_id     INTEGER PRIMARY KEY NOT NULL REFERENCES users(id),
    global_wish TEXT
);

CREATE TABLE ss_events (
    id          INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    name        TEXT    NOT NULL,
    description TEXT,
    host_id     INTEGER NOT NULL REFERENCES users(id),
    status      INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE ss_participants (
    event_id    INTEGER NOT NULL REFERENCES ss_events(id) ON DELETE CASCADE,
    user_id     INTEGER NOT NULL REFERENCES users(id),
    event_wish  TEXT,
    assignee_id INTEGER REFERENCES users(id),
    PRIMARY KEY (event_id, user_id)
);
```

Notes:

- sqlx stores a checksum for every applied migration. After editing this file, rebuild the
  local database with `sqlx database reset -y`. Otherwise both `sqlx migrate run` and the
  bot's startup `migrate!` fail with a checksum mismatch.
- `sqlx::query!` is checked at compile time against the local database. The schema rewrite and
  the query updates in `SqliteSecretSantaRepo` must land in the **same commit**.
- **Once the bot is deployed, this changes:** migrations become append-only, and any later
  schema change gets a new migration file.
- **Table ownership convention:** `framework/` owns unprefixed tables (`users`). Each feature
  owns tables with its own prefix (`ss_*`) and may reference `users(id)`, but never another
  feature's tables.

## Framework API

### `Feature` trait

```rust
// framework/feature.rs
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait Feature: Send + Sync {
    /// Human-readable name, used in logs.
    fn name(&self) -> &'static str;

    /// Prefix of every custom_id this feature emits, e.g. "ss" → "ss:btn:start:5".
    /// The registry routes components and modals by this prefix.
    fn namespace(&self) -> &'static str;

    /// Top-level slash commands this feature owns. The registry routes by `CreateCommand` name.
    fn commands(&self) -> Vec<CreateCommand> { vec![] }

    async fn on_command(&self, _ctx: &InteractionCtx, _req: CommandRequest) -> Result<(), FeatureError> { Ok(()) }
    async fn on_component(&self, _ctx: &InteractionCtx, _req: ComponentRequest) -> Result<(), FeatureError> { Ok(()) }
    async fn on_modal(&self, _ctx: &InteractionCtx, _req: ModalRequest) -> Result<(), FeatureError> { Ok(()) }
}
```

- The default methods mean a feature implements only the hooks it uses.
- Use `async_trait` rather than native `async fn` in traits, because the registry stores
  `Arc<dyn Feature>` and native async trait methods can't be used through `dyn`.
- Put `#[automock]` above `#[async_trait]`.

### Request and context types

```rust
// framework/request.rs  — plain data, trivially constructible in tests
pub struct CommandRequest {
    pub user: UserId,
    pub guild: Option<GuildId>,
    pub command: String,            // "ss"
    pub subcommand: Option<String>, // "info"
    pub options: Options,           // flattened: options of the subcommand if there is one
}
pub struct ComponentRequest { pub user: UserId, pub custom_id: String, pub kind: ComponentKind }
pub enum ComponentKind { Button, UserSelect(Vec<UserId>), StringSelect(Vec<String>) }
pub struct ModalRequest { pub user: UserId, pub custom_id: String, pub fields: HashMap<String, String> }

impl Options {
    pub fn i64(&self, name: &str) -> Option<i64>;
    pub fn str(&self, name: &str) -> Option<&str>;
    pub fn user(&self, name: &str) -> Option<UserId>;
}

// framework/context.rs
pub struct InteractionCtx {
    pub responder: Arc<dyn Responder>,
    pub discord: Arc<dyn DiscordApi>,
    pub users: Arc<dyn UserRepo>,
}
```

Serenity's id types (`UserId`, `GuildId`) are reused as they are, since they're
simple newtypes.

### Discord traits

```rust
// framework/discord.rs
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait Responder: Send + Sync {
    async fn respond(&self, response: CreateInteractionResponse) -> Result<(), DiscordError>;
    async fn followup(&self, message: CreateInteractionResponseFollowup) -> Result<(), DiscordError>;
    fn has_responded(&self) -> bool;
}

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait DiscordApi: Send + Sync {
    async fn send_dm(&self, user: UserId, content: String) -> Result<(), DiscordError>;
    async fn user_name(&self, user: UserId) -> Result<String, DiscordError>;
}

// Real impls
pub struct SerenityResponder { http: Arc<Http>, id: InteractionId, token: String, responded: AtomicBool }
pub struct SerenityDiscordApi { http: Arc<Http> }
```

`SerenityResponder::respond` calls `response.execute(&http, (id, &token))`, using serenity's
`Builder` trait, and sets `responded`.

### Shared users

```rust
// framework/users.rs
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait UserRepo: Send + Sync {
    /// Insert any of `users` not already present (INSERT OR IGNORE). Idempotent.
    async fn ensure(&self, users: &[UserId]) -> Result<(), sqlx::Error>;
}

pub struct SqliteUserRepo { pool: SqlitePool }
```

- The registry calls `users.ensure(&[req.user])` before dispatching any interaction, so the
  user who triggered it always has a `users` row.
- A feature that references *other* users must call `ctx.users.ensure(..)` before its own
  writes, so the foreign keys are satisfied. The Secret Santa participant select menu is an
  example.
- Future per-user settings (timezone, locale, opt-outs) become columns on `users` and methods
  on `UserRepo`.

### Registry

```rust
// framework/registry.rs
pub struct FeatureRegistry {
    features: Vec<Arc<dyn Feature>>,
    by_command: HashMap<String, usize>,
    by_namespace: HashMap<&'static str, usize>,
    users: Arc<dyn UserRepo>,
}

impl FeatureRegistry {
    pub fn builder(users: Arc<dyn UserRepo>) -> RegistryBuilder;
    pub fn commands(&self) -> Vec<CreateCommand>;                 // all features, for set_global_commands
    pub async fn dispatch(&self, http: Arc<Http>, interaction: Interaction);
}

impl RegistryBuilder {
    pub fn register(self, feature: impl Feature + 'static) -> Self;
    /// Fails on a duplicate command name or namespace, so a clash is caught at startup.
    pub fn build(self) -> Result<FeatureRegistry, RegistryError>;
}
```

`dispatch` does five things:

1. Converts the serenity interaction into a request struct.
2. Ensures the user has a `users` row.
3. Finds the owning feature by command name, or by the custom-id prefix before the first `:`.
4. Builds `InteractionCtx` and awaits the handler.
5. Handles `Err`:
   - `FeatureError::User(msg)`: sends an ephemeral `msg` through `respond` or `followup`,
     depending on `has_responded()`.
   - `FeatureError::Internal(e)`: logs `e` and sends a generic "Something went wrong".

Other interaction types (autocomplete, ping) are logged and ignored.

### Errors

```rust
// framework/error.rs
pub enum FeatureError {
    User(String),                                       // safe to show the user
    Internal(Box<dyn std::error::Error + Send + Sync>), // logged, generic reply
}
```

Each feature keeps its own error enum (e.g. `SsError::NotHost`, `SsError::EventNotFound`,
`SsError::Repo(sqlx::Error)`) with `impl From<SsError> for FeatureError`. This maps user-facing
variants to `User` and everything else to `Internal`. Handlers then use `?` everywhere, which
replaces the roughly 40 `.expect()` calls that panic the handler task today.

## main.rs After the Refactor

```rust
use jabot::framework::{FeatureRegistry, SqliteUserRepo};
use jabot::features::secret_santa::{SecretSanta, SqliteSecretSantaRepo};

struct Bot {
    registry: FeatureRegistry,
}

#[async_trait]
impl EventHandler for Bot {
    async fn ready(&self, ctx: Context, ready: Ready) {
        tracing::info!("{} is connected", ready.user.name);
        // One call replaces the whole global set, so commands removed from code (e.g. /ss join) disappear from Discord.
        if let Err(e) = Command::set_global_commands(&ctx.http, self.registry.commands()).await {
            tracing::error!("register commands: {e}");
        }
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        self.registry.dispatch(ctx.http.clone(), interaction).await;
    }
}

#[tokio::main]
async fn main() {
    let token = get_discord_token().expect("Failed to get Discord token");
    let pool = connect_database().await;                  // SqlitePoolOptions + migrate!

    let registry = FeatureRegistry::builder(Arc::new(SqliteUserRepo::new(pool.clone())))
        .register(SecretSanta::new(Arc::new(SqliteSecretSantaRepo::new(pool.clone()))))
        // .register(Birthdays::new(Arc::new(SqliteBirthdayRepo::new(pool.clone()))))
        .build()
        .expect("Feature registration conflict");

    // Interactions (commands, components, modals) arrive without any gateway intents.
    let mut client = Client::builder(&token, GatewayIntents::empty())
        .event_handler(Bot { registry })
        .await
        .expect("Err creating client");
    client.start().await.expect("Client error");
}
```

## Secret Santa Feature Internals

### Entry point: routing only

```rust
// features/secret_santa/mod.rs
pub struct SecretSanta { repo: Arc<dyn SecretSantaRepo> }

#[async_trait]
impl Feature for SecretSanta {
    fn name(&self) -> &'static str { "Secret Santa" }
    fn namespace(&self) -> &'static str { "ss" }
    fn commands(&self) -> Vec<CreateCommand> { vec![commands::ss_command()] }

    async fn on_command(&self, ctx: &InteractionCtx, req: CommandRequest) -> Result<(), FeatureError> {
        match req.subcommand.as_deref() {
            Some(text::CMD_CREATE) => commands::create(self, ctx, &req).await,
            Some(text::CMD_INFO)   => commands::info(self, ctx, &req).await,
            Some(text::CMD_LIST)   => commands::list(self, ctx, &req).await,
            _ => Err(SsError::UnknownSubcommand),
        }.map_err(Into::into)
    }

    async fn on_component(&self, ctx: &InteractionCtx, req: ComponentRequest) -> Result<(), FeatureError> {
        match req.custom_id.parse::<SsId>()? {
            SsId::UserSelect(id) => components::set_participants(self, ctx, id, &req).await,
            SsId::Start(id)      => components::start(self, ctx, id, req.user, &mut rand::rng()).await,
            SsId::End(id)        => components::end(self, ctx, id, req.user).await,
            SsId::Cancel(id)     => components::cancel(self, ctx, id, req.user).await,
            other                => Err(SsError::UnexpectedId(other)),
        }.map_err(Into::into)
    }

    async fn on_modal(&self, ctx: &InteractionCtx, req: ModalRequest) -> Result<(), FeatureError> {
        match req.custom_id.parse::<SsId>()? {
            SsId::CreateModal   => modals::create(self, ctx, &req).await,
            SsId::EditModal(id) => modals::edit(self, ctx, id, &req).await,
            other               => Err(SsError::UnexpectedId(other)),
        }.map_err(Into::into)
    }
}
```

### Repository trait

Methods are shaped around use cases, so operations that must happen together run in one
transaction inside the sqlx impl. State changes are **conditional**
(`UPDATE ... WHERE id = ? AND status = ?`) and report whether they happened. This makes a
double click harmless (see bug B5).

```rust
// features/secret_santa/repo.rs
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait SecretSantaRepo: Send + Sync {
    async fn get_event(&self, id: EventId) -> Result<Option<Event>, sqlx::Error>;
    async fn events_for_user(&self, user: UserId) -> Result<Vec<Event>, sqlx::Error>;
    async fn count_active_hosted(&self, host: UserId) -> Result<i64, sqlx::Error>;
    async fn create_event(&self, host: UserId, name: &str, desc: &str) -> Result<EventId, sqlx::Error>; // event + host participant, one tx
    async fn update_event(&self, id: EventId, name: &str, desc: Option<&str>) -> Result<(), sqlx::Error>;
    async fn participants(&self, id: EventId) -> Result<Vec<Participant>, sqlx::Error>;
    async fn set_participants(&self, id: EventId, add: &[UserId], remove: &[UserId]) -> Result<(), sqlx::Error>; // one tx
    /// PreRun → Running and writes all assignments, one tx. Ok(false) if the event was no longer PreRun.
    async fn start_event(&self, id: EventId, assignments: &[(UserId, UserId)]) -> Result<bool, sqlx::Error>;
    /// Conditional status change. Ok(false) if the current status isn't `from`.
    async fn transition(&self, id: EventId, from: &[EventStatus], to: EventStatus) -> Result<bool, sqlx::Error>;
}
```

- `SqliteSecretSantaRepo` keeps using compile-checked `sqlx::query!`.
- The dynamic bulk insert and delete move to `sqlx::QueryBuilder`.

### Keeping handlers thin

Each handler follows the same five steps:

1. Parse the input from the request.
2. Load state through `repo`.
3. Decide using pure functions in `rules.rs`.
4. Persist through `repo` (and `ctx.users.ensure` for any newly referenced users).
5. `ctx.responder.respond(views::…)`, **then** `ctx.discord.send_dm(..)` for notifications.

The reply goes out before the DMs because Discord allows only 3 seconds to acknowledge an
interaction. A mockall `Sequence` checks this ordering in tests.

For DM failures (e.g. a user with DMs closed), the handler **logs the failure and continues**
with the rest. It then sends the host one `followup` listing who couldn't be notified (see
bug B6).

Randomness is passed in as `&mut impl rand::Rng`. Production uses `rand::rng()`; tests use a
seeded `StdRng`.

## Bug Fixes

Every fix follows the same pattern:

1. Write a test that fails against the current behavior.
2. Fix the bug.
3. Commit, with the bug ID in the message (e.g. `fix(ss): B1 scope assignment update to event`).

The step column refers to [Migration Steps](#migration-steps).

| ID | Bug | Step |
|---|---|---|
| B1 | Starting an event overwrites assignments in the user's other events | 5 |
| B2 | Starting an event is not atomic | 5 |
| B3 | Participant select menu has no host or existence check | 10 |
| B4 | Host can remove themselves; events over 25 users lose participants | 10 |
| B5 | Double-clicking Start/End/Cancel runs the action twice | 5 |
| B6 | A failed DM aborts notification loops | 9 |
| B7 | `/ss list` makes one HTTP call per event | 10 |
| B8 | Invalid status values silently become PreRun | 8 |
| B9 | `DATABASE_URL` is used as a file path | 1 |
| B10 | Misspelled database file is committed to git | 1 |
| — | `/ss join` is broken; Leave handler reassigns users to themselves | 2 (removed) |

### B1: Starting an event overwrites assignments in the user's other events

- **Problem:** `main.rs:724` runs
  `UPDATE event_participants SET assignee_id = ? WHERE user_id = ?` with no `event_id` filter.
  A user in two events gets the new assignee written into **both** events.
- **Fix:** `SqliteSecretSantaRepo::start_event` updates `WHERE event_id = ? AND user_id = ?`.
- **Test:** `#[sqlx::test]` where user U is in running event A with assignee X. Starting event B
  leaves A's assignment for U as X.

### B2: Starting an event is not atomic

- **Problem:** the status update and each assignment write are separate statements, and they
  are interleaved with DMs that `.expect()`. If one DM fails, the task panics. The event is
  then left `Running`, with only some participants assigned or notified.
- **Fix:** `start_event` writes the status and all assignments in one transaction, **before**
  any DMs are sent. DMs happen afterwards and follow B6.
- **Test:** a mock test in which `send_dm` fails for the first participant: `start_event` has
  already been called once with the full assignment list. A `#[sqlx::test]` shows a failing
  assignment write rolls back the status.

### B3: Participant select menu has no host or existence check

- **Problem:** the user-select handler (`main.rs:565`) never checks that the person clicking is
  the host. If the event is missing, it panics on `fetch_one(..).expect(..)`.
- **Fix:** `set_participants` loads the event and returns `SsError::EventNotFound` or
  `SsError::NotHost` before making any change.
- **Test:** a mock test where a non-host submits the select: `set_participants` on the repo runs
  `.times(0)` and the reply is "You are not the host of this event".

### B4: Host can remove themselves; events over 25 users lose participants

- **Problem:** the select diff (`main.rs:621-639`) removes everyone who isn't selected,
  including the host.
  - Separately, the menu allows at most 25 values. An event with more than 25 participants
    can't be shown in full, so saving the menu silently removes the extras.
- **Fix:** a pure function `rules::participant_diff(existing, selected, host) -> (add, remove)`
  always keeps the host.
  - `set_participants` rejects any change that would take the event over 25 participants
    (`SsError::TooManyParticipants`), so the menu can always show everyone.
- **Test:** plain unit tests on `participant_diff` (host deselected → not in `remove`), plus a
  mock test for the limit.

### B5: Double-clicking Start/End/Cancel runs the action twice

- **Problem:** each handler checks the status, then updates it, as separate steps. Two quick
  clicks both pass the check. For Start, this shuffles twice and DMs everyone two different
  assignments.
- **Fix:** conditional updates (`start_event` / `transition` return `Ok(false)` when the status
  has already changed). When that happens, the handler replies "Event was already started" or
  a similar message and sends no DMs.
- **Test:** `#[sqlx::test]` calling `start_event` twice: the second returns `false` and the
  assignments are unchanged. A mock test covers the `false` case: no `send_dm` calls.

### B6: A failed DM aborts notification loops

- **Problem:** invite, start and cancel DMs use `.expect()`. One user with DMs closed panics
  the task, so everyone after them in the loop is never notified.
- **Fix:** log each failure and continue. Afterwards, send the host one ephemeral `followup`:
  "Couldn't DM: @a, @b (they may have DMs disabled)".
- **Test:** a mock test in which `send_dm` fails for user 2 of 3: user 3 still gets a DM, and
  `followup` is called once and mentions user 2.

### B7: `/ss list` makes one HTTP call per event

- **Problem:** `/ss list` calls `to_user` for each event's host (`main.rs:308`). This is slow
  and risks rate limits.
- **Fix:** render the host as a `<@id>` mention, as `info` already does. Discord shows the name
  on the client side and no API call is needed. This also removes `DiscordApi::user_name` from
  this path.
- **Test:** a view test showing the list output contains `<@{host}>`, and a mock test showing
  `user_name` runs `.times(0)`.

### B8: Invalid status values silently become PreRun

- **Problem:** `From<i32> for SSState` maps any unknown value to `PreRun`. A corrupted row
  would look startable.
- **Fix:** use `TryFrom<i64> for EventStatus` and map an unknown value to an `Internal` error.
- **Test:** a plain unit test of the conversion table.

### B9: `DATABASE_URL` is used as a file path

- **Problem:** `main.rs:1070` passes `DATABASE_URL` (`sqlite:database.sqlite`) to
  `SqliteConnectOptions::filename(..)`, which treats the whole string as a path. The bot would
  open a file literally named `sqlite:database.sqlite` rather than the database the sqlx CLI
  set up and checked the queries against.
- **Fix:** `SqliteConnectOptions::from_str(&url)?.create_if_missing(true)`.
- **Test:** a small unit test that `connect_options("sqlite:foo.sqlite")` resolves to the
  filename `foo.sqlite`.

### B10: Misspelled database file is committed to git

- **Problem:** `databse.sqlite` (misspelled) is tracked, but `.gitignore` only covers
  `database.sqlite`.
- **Fix:** `git rm databse.sqlite` (it's empty) and change the `.gitignore` entry to
  `*.sqlite`.

### Removed instead of fixed

These go away with the auto-join decision:

- **`/ss join` is broken.** It reads the subcommand option instead of `id`, and it was never
  registered. It is deleted along with `user_join_event`.
- **The Leave handler** reassigns the remaining participant to themselves in a 2-person event,
  and `unwrap()`s a missing assignee. It is deleted along with the commented-out Leave button.
  If leaving comes back, `rules::reassign_on_leave` must refuse when fewer than 3 participants
  would remain.

## Testing Strategy

| What | How | Examples |
|---|---|---|
| `rules.rs` | plain `#[test]` | `assign_santas` produces one cycle with no self-assignment for n ≥ 2; transition table; `participant_diff` keeps the host |
| Handlers | `MockSecretSantaRepo` + `MockResponder` + `MockDiscordApi` + `MockUserRepo` | non-host start → `NotHost`, `start_event` `.times(0)`; event limit reached → no insert; cancel-while-running DMs everyone, cancel-while-preparing DMs nobody |
| `SqliteSecretSantaRepo`, `SqliteUserRepo` | `#[sqlx::test]` (in-memory SQLite + migrations) | the mocks never run SQL, so the queries need their own tests; B1, B2 and B5 regressions live here |
| `custom_id.rs`, `views.rs` | plain `#[test]` + `serde_json::to_value` | `parse(format(id)) == id`; info view shows Start/Cancel only for the host in PreRun |
| `FeatureRegistry` | `MockFeature` + `MockUserRepo` | routes `"ss:btn:start:5"` by namespace; `build()` rejects duplicates; `User` error → ephemeral reply; `ensure` called before dispatch |

Example handler test, including the reply-before-DMs check:

```rust
#[tokio::test]
async fn cancel_running_event_replies_then_dms_participants() {
    let host = UserId::new(1);
    let mut repo = MockSecretSantaRepo::new();
    repo.expect_get_event()
        .with(eq(EventId(5)))
        .returning(move |_| Ok(Some(Event { host, status: EventStatus::Running, ..fixture() })));
    repo.expect_transition()
        .withf(|id, from, to| *id == EventId(5) && from.contains(&EventStatus::Running) && *to == EventStatus::Finished)
        .times(1)
        .returning(|_, _, _| Ok(true));
    repo.expect_participants()
        .returning(move |_| Ok(vec![participant(host), participant(UserId::new(2))]));

    let mut seq = Sequence::new();
    let mut responder = MockResponder::new();
    responder.expect_respond()
        .withf(|r| serde_json::to_value(r).unwrap()["data"]["content"] == "Event canceled successfully!")
        .times(1).in_sequence(&mut seq)
        .returning(|_| Ok(()));
    let mut discord = MockDiscordApi::new();
    discord.expect_send_dm().times(2).in_sequence(&mut seq).returning(|_, _| Ok(()));

    let ss = SecretSanta::new(Arc::new(repo));
    let ctx = InteractionCtx {
        responder: Arc::new(responder),
        discord: Arc::new(discord),
        users: Arc::new(MockUserRepo::new()), // no expectations: any call fails the test
    };

    ss.on_component(&ctx, ComponentRequest::button(host, "ss:btn:cancel:5")).await.unwrap();
}
```

**Mock visibility:** `#[cfg_attr(test, automock)]` only generates mocks for unit tests inside
the crate, including tests inside feature modules. If integration tests in `tests/` also need
them, add a `mocks` cargo feature and use `#[cfg_attr(any(test, feature = "mocks"), automock)]`.

## Adding a Feature Later (Checklist)

1. Create `src/features/<name>/` (or `<name>.rs` if it's small) with a struct that implements
   `Feature`, and pick a unique `namespace()`.
2. If it needs storage, define a `<Name>Repo` trait with `#[cfg_attr(test, automock)]` and a
   sqlx impl.
   - Add a migration for `<prefix>_*` tables.
   - Reference `users(id)` for any Discord user.
3. Add one `.register(...)` line in `main.rs`.
4. Write handler tests with the mocks. The framework already provides `MockResponder`,
   `MockDiscordApi` and `MockUserRepo`.

## Adding Message Support Later

This is deliberately left out for now. When a feature needs to react to messages (or to other
gateway events such as reactions or member joins), extend the framework in one commit:

1. **Opt in per feature.** Add `fn intents(&self) -> GatewayIntents { GatewayIntents::empty() }`
   to `Feature`. `main.rs` passes `registry.intents()`, the combination across all features,
   to `Client::builder`, so the bot only requests what its registered features use.
2. **Hook.** Add `async fn on_message(&self, ctx: &MessageCtx, msg: MessageRequest)` with a
   default no-op. `MessageRequest` holds the author, channel, guild and content;
   `MessageCtx` holds `discord` and `users`. Add `DiscordApi::send_message(channel, text)` for
   replies.
3. **Dispatch.** `Bot::message` calls `registry.dispatch_message(..)`. It skips messages from
   bots (including this one) and calls `on_message` only on features whose `intents()`
   includes message events. It logs errors, since there's no interaction to reply to.
   - Don't call `users.ensure` for messages; that would mean a database write for every chat
     message.
4. **Restricted intents.** Reading message text in servers needs `MESSAGE_CONTENT`, which is
   restricted:
   - Enable it in the Developer Portal first, or the bot fails to connect ("Disallowed
     intents").
   - Once the bot is in 100+ servers, Discord requires verification and a written reason for
     needing it.
   - DMs to the bot and messages that mention it include their text without it.

## Cargo.toml Changes

```toml
[dependencies]
async-trait = "0.1"
thiserror = "2"
tracing = "0.1"
tracing-subscriber = "0.3"

[dev-dependencies]
mockall = "0.13"
serde_json = "1"
```

`sqlx`'s default `macros` and `migrate` features cover `#[sqlx::test]`. The existing
`tokio = { features = ["macros", "rt-multi-thread"] }` covers `#[tokio::test]`.

## Migration Steps

Each step should compile and pass `cargo test`. Steps 1–4 and 6–8 don't change behavior; the
bug fixes land in steps 1, 5, 9 and 10. Commit after each step (and after each bug fix within
a step).

1. **Repo hygiene**: B10 (delete the committed database file, `*.sqlite` in `.gitignore`),
   then B9 (fix the connection string).
2. **Trim scope** (still in `main.rs`): delete `/ss join`, `user_join_event`, the Leave
   handler and the commented-out Leave button, the unused `Strings` constants, the dead
   `Handler` block, and the `joined = TRUE` filters in queries. Delete the empty `message`
   handler and switch the client to `GatewayIntents::empty()`.
3. **Framework and lift-and-shift**:
   - Add `lib.rs` and `framework/`, containing `Feature`, the request types,
     `Responder`/`DiscordApi` with their serenity impls, `FeatureRegistry` and
     `FeatureError`. Add registry tests with `MockFeature`.
   - Create `features/secret_santa/mod.rs` with `SecretSanta { pool }` and move the handler
     code in with as few changes as possible:
     - `command.create_response(&ctx.http, r)` becomes `ctx.responder.respond(r)`.
     - `user.direct_message(..)` becomes `ctx.discord.send_dm(..)`.
   - Switch command registration to `set_global_commands`.
4. **Typed custom IDs**: add the `SsId` enum with `Display`/`FromStr` and round-trip tests.
   This replaces `contains()`/`splitn()`.
5. **Repository**: add the `SecretSantaRepo` trait and `SqliteSecretSantaRepo` (still on the old
   table names); swap `SecretSanta { pool }` for `SecretSanta { repo }`; add `#[sqlx::test]`
   coverage.
   - B1, B2 and B5 are fixed here. The transactional, conditional `start_event`/`transition`
     methods are correct by design, so there is no point reproducing the buggy SQL. Land the
     regression tests in the same commit and name the bug IDs in the message.
6. **Schema rewrite**: replace the initial migration with the one in [Schema](#schema) and run
   `sqlx database reset -y`; add `framework/users.rs` (`UserRepo`, `SqliteUserRepo`) and the
   registry's `ensure` call; add `users` to `InteractionCtx`; point
   `SqliteSecretSantaRepo` at the `ss_*` tables.
7. **Split handlers** into `commands.rs`, `components.rs`, `modals.rs` and `views.rs`, one
   handler per commit if you like. Each move adds its mockall tests.
8. **Pure rules and model**: move the assignment shuffle, host checks and transitions into
   `rules.rs`; `EventStatus` gets `TryFrom` (**B8**).
9. **Errors**: add `SsError` → `FeatureError`; replace the `.expect()` calls with `?`; switch
   `println!` to `tracing`. The DM-loop change (**B6**) lands here, since it's the same code.
10. **Remaining bug fixes**, one commit each, failing test first: **B3**, **B4**, **B7**.
11. **Docs**: rewrite the Architecture section of `CLAUDE.md`. Cover the framework and feature
    layout, the table-prefix convention, the "adding a feature" checklist, and the need to run
    `sqlx migrate run` before building.
