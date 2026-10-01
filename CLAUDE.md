# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

JABot is a Rust Discord bot with features such as orchestrating a secret santa. It uses the Serenity Discord API library and SQLx for database operations with SQLite. Each capability is a self-contained *feature* plugged into a small framework, and features take their dependencies as traits so they can be tested with `mockall`.

## Database Setup

SQLx requires database access at compile time for query validation. Before building:

1. Install SQLx CLI: https://github.com/launchbadge/sqlx/tree/main/sqlx-cli (`cargo install sqlx-cli --no-default-features --features sqlite,rustls`)
2. Create `.env` file with: `DATABASE_URL=sqlite:database.sqlite`
3. Run `sqlx database setup` to create the database and apply migrations from `migrations/`

After a migration changes, run `sqlx migrate run` before `cargo build`, or the `query!` macros check against a stale schema. The bot is deployed, so migrations are append-only: add a new file (`sqlx migrate add <name>`) and never edit an applied one (sqlx stores a checksum per migration, and the bot refuses to start on a mismatch). The bot applies pending migrations itself at startup.

After adding or changing a `query!`, run `cargo sqlx prepare -- --all-targets` and commit `.sqlx/`. The Docker build compiles with `SQLX_OFFLINE=true` against that data, without a database.

## Development Commands

- **Build**: `cargo build`
- **Run**: `cargo run` (requires `DISCORD_TOKEN_FILE` environment variable pointing to file with Discord API token; add `HTTP_PORT=8080` to enable the HTTP server and `tell`)
- **Run with explicit env**: `DATABASE_URL=sqlite:database.sqlite DISCORD_TOKEN_FILE=... cargo run`
- **Test**: `cargo test` (`#[sqlx::test]` tests create their own throwaway databases under `target/sqlx/`)
- **Lint**: `cargo clippy --all-targets`
- **Deploy** (on the server, from the deployment checkout): `./deploy.sh` pulls, rebuilds the image and restarts the container with `docker compose up -d --build`. Setup is in the README ("Deployment")

## Environment Variables

- `DATABASE_URL`: Required for SQLx, typically `sqlite:database.sqlite` (parsed as a URL, not a path)
- `DISCORD_TOKEN_FILE`: Path to file containing Discord API token
- `RUST_LOG`: Log filter for `tracing-subscriber`, e.g. `info` or `jabot=debug` (`debug` logs each HTTP request's peer address)
- `HTTP_PORT`: Port for the HTTP server; unset disables it, and `tell` with it
- `BASE_PATH`: Prefix of every HTTP route (default `/jabot`); the front proxy forwards `/jabot/*` unchanged
- `TRUSTED_PROXIES`: Comma-separated IPs/CIDR ranges allowed to set `X-Forwarded-For` (default empty: the header is ignored)
- `DOMAIN`, `LAN_HOST`: Host names for the URLs `/tell` prints (`https://<host><BASE_PATH>/tell`); without `DOMAIN` it prints `http://localhost:<port>...`

Under Docker Compose, `compose.yaml` sets these from `.env` (see `.env.example`), plus `HTTP_PUBLISH`, the host address the port is published on.

## Dependencies

- `serenity = "0.12"` - Discord API library
- `sqlx = { version = "0.8", features = ["runtime-tokio-rustls", "sqlite"] }` - Database toolkit
- `axum = "0.8"`, `ipnet`, `serde` - HTTP server, trusted proxy ranges, JSON request bodies
- `async-trait`, `thiserror`, `tracing` - trait objects with async methods, error types, logging
- `mockall` (dev) - mocks generated from the dependency traits
- `tower` (dev) - `ServiceExt::oneshot` for HTTP tests against the real router

## Architecture

```
src/
├── main.rs        # config, SqlitePool + migrations, serenity Client, Bot (EventHandler), feature registration, HTTP server
├── lib.rs
├── framework/     # shared plumbing; knows nothing about any feature
│   ├── feature.rs   # trait Feature: name, namespace, commands, on_command/on_component/on_modal, http_routes
│   ├── registry.rs  # FeatureRegistry: routes commands by name, components/modals by custom-id namespace; builds the HTTP router
│   ├── http.rs      # HttpCtx, HttpConfig, serve, ClientIp (X-Forwarded-For from trusted proxies only)
│   ├── request.rs   # CommandRequest/ComponentRequest/ModalRequest: plain data built from serenity interactions
│   ├── context.rs   # InteractionCtx { responder, discord, users }
│   ├── discord.rs   # traits Responder (reply to this interaction) + DiscordApi (DMs, lookups), serenity impls; DmError
│   ├── users.rs     # trait UserRepo + SqliteUserRepo: the shared `users` table
│   └── error.rs     # FeatureError::User (shown to the user) / Internal (logged, generic reply)
└── features/
    ├── tell/            # /tell gives a token; POST <BASE_PATH>/tell with it DMs the owner
    │   ├── mod.rs         # Tell: impl Feature, routing only; TellConfig
    │   ├── commands.rs    # /tell definition and handler
    │   ├── components.rs  # Revoke token button
    │   ├── custom_id.rs   # TellId: `tell:revoke:<token id>`
    │   ├── http.rs        # axum handler: extract, TellService::deliver, TellError → status
    │   ├── service.rs     # rate limits, token lookup, message checks, DM
    │   ├── rate_limit.rs  # RateLimiter<K>: in-memory token buckets (GCRA)
    │   ├── token.rs       # token generation
    │   ├── model.rs       # TokenId, StoredToken, TellRequest, TellError
    │   ├── repo.rs        # trait TellRepo + SqliteTellRepo (tell_* SQL)
    │   ├── views.rs, text.rs
    │   └── tests.rs       # command/button tests against mocks
    └── secret_santa/
        ├── mod.rs         # SecretSanta: impl Feature, routing only
        ├── commands.rs    # /ss definition and create/info/list handlers
        ├── components.rs  # participant picker, start/end/cancel buttons
        ├── modals.rs      # create/edit form submissions
        ├── custom_id.rs   # SsId: typed `ss:<action>[:<event id>]` custom ids
        ├── model.rs       # EventId, EventStatus, Event, Participant, SsError
        ├── rules.rs       # pure business rules (host checks, transitions, assignment draw)
        ├── repo.rs        # trait SecretSantaRepo + SqliteSecretSantaRepo (all ss_* SQL)
        ├── views.rs       # responses, forms and DM text
        ├── text.rs        # command names and user-facing strings
        └── tests.rs       # handler tests against mocks
```

How an interaction flows: `Bot::interaction_create` → `FeatureRegistry::dispatch` converts it to a request struct, ensures the user has a `users` row, picks the feature (by command name, or by the custom-id prefix before the first `:`), and calls its hook with an `InteractionCtx`. A handler loads state through its repository, decides with pure functions in `rules.rs`, persists, replies with `ctx.responder`, and only then sends DMs with `ctx.discord` (Discord allows 3 seconds to acknowledge). Returning an error lets the registry reply: refusals are shown ephemerally, everything else is logged.

How an HTTP request flows: `main.rs` runs `serve` beside the Discord client (whichever stops first, or SIGTERM, ends the process). `FeatureRegistry::http_router` nests each feature's `http_routes` under `<BASE_PATH>/<namespace>` and adds `<BASE_PATH>/healthz`, a 32 KiB body limit and request logging. Handlers get an `HttpCtx { discord, users }` and extract `ClientIp`; they return their own error type implementing `IntoResponse`. TLS is the front proxy's job; the bot speaks plain HTTP.

Conventions:
- Feature code never touches serenity's `Context`/`Http` or a `SqlitePool` directly; it goes through `InteractionCtx`/`HttpCtx` and its own repository trait, so every handler is testable with mocks (`MockResponder`, `MockDiscordApi`, `MockUserRepo`, `MockSecretSantaRepo`). Put `#[cfg_attr(test, mockall::automock)]` above `#[async_trait]`.
- Repository methods are shaped around use cases; writes that belong together run in one transaction, and status changes are conditional (`... WHERE status = ?`) so double clicks are harmless.
- Tables: `framework/` owns unprefixed tables (`users`); each feature owns tables with its own prefix (`ss_*`) and may reference `users(id)`, never another feature's tables. Write any user id to a feature table only after `ctx.users.ensure(..)`.
- The bot requests no gateway intents; interactions don't need them.

### Adding a feature

1. Create `src/features/<name>/` (or `<name>.rs` if small) with a struct implementing `Feature`, and pick a unique `namespace()`; every custom id it emits starts with `<namespace>:`.
2. If it needs storage, define a `<Name>Repo` trait with `#[cfg_attr(test, mockall::automock)]` and a sqlx impl, and add a migration for `<prefix>_*` tables that reference `users(id)`.
3. Add one `.register(...)` line in `main.rs`. `build()` fails at startup on a duplicate command name or namespace.
4. Test handlers with the mocks (`crate::framework::testing` has helpers for building a context and reading responses).
5. For HTTP endpoints, implement `http_routes`; routes are served under `<BASE_PATH>/<namespace>`. Keep handlers thin (extract, call a service, map errors) and test the service with mocks and the routes with `tower::ServiceExt::oneshot` plus `MockConnectInfo` (see `features/tell/http.rs`). The front proxy decides which paths are public.

Message events aren't supported yet; `REFACTOR_PLAN.md` ("Adding Message Support Later") describes how to add them.
