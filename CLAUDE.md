# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

JABot is a Rust Discord bot with features such as orchestrating a secret santa. It uses the Serenity Discord API library and SQLx for database operations with SQLite. Each capability is a self-contained *feature* plugged into a small framework, and features take their dependencies as traits so they can be tested with `mockall`.

## Database Setup

SQLx requires database access at compile time for query validation. Before building:

1. Install SQLx CLI: https://github.com/launchbadge/sqlx/tree/main/sqlx-cli (`cargo install sqlx-cli --no-default-features --features sqlite,rustls`)
2. Create `.env` file with: `DATABASE_URL=sqlite:database.sqlite`
3. Run `sqlx database setup` to create the database and apply migrations from `migrations/`

After a migration changes, run `sqlx migrate run` before `cargo build`, or the `query!` macros check against a stale schema. The bot isn't deployed yet, so the initial migration is still edited in place; after editing it, run `sqlx database reset -y` (sqlx stores a checksum per applied migration). Once deployed, migrations become append-only.

## Development Commands

- **Build**: `cargo build`
- **Run**: `cargo run` (requires `DISCORD_TOKEN_FILE` environment variable pointing to file with Discord API token)
- **Run with explicit env**: `DATABASE_URL=sqlite:database.sqlite DISCORD_TOKEN_FILE=... cargo run`
- **Test**: `cargo test` (`#[sqlx::test]` tests create their own throwaway databases under `target/sqlx/`)
- **Lint**: `cargo clippy --all-targets`
- **Deploy** (on the server): `./deploy.sh` builds a release binary, stops the `jabot` service, installs the binary over its `ExecStart` path and starts it again

## Environment Variables

- `DATABASE_URL`: Required for SQLx, typically `sqlite:database.sqlite` (parsed as a URL, not a path)
- `DISCORD_TOKEN_FILE`: Path to file containing Discord API token
- `RUST_LOG`: Log filter for `tracing-subscriber`, e.g. `info` or `jabot=debug`

## Dependencies

- `serenity = "0.12"` - Discord API library
- `sqlx = { version = "0.8", features = ["runtime-tokio-rustls", "sqlite"] }` - Database toolkit
- `async-trait`, `thiserror`, `tracing` - trait objects with async methods, error types, logging
- `mockall` (dev) - mocks generated from the dependency traits

## Architecture

```
src/
├── main.rs        # config, SqlitePool + migrations, serenity Client, Bot (EventHandler), feature registration
├── lib.rs
├── framework/     # shared plumbing; knows nothing about any feature
│   ├── feature.rs   # trait Feature: name, namespace, commands, on_command/on_component/on_modal
│   ├── registry.rs  # FeatureRegistry: routes commands by name, components/modals by custom-id namespace
│   ├── request.rs   # CommandRequest/ComponentRequest/ModalRequest: plain data built from serenity interactions
│   ├── context.rs   # InteractionCtx { responder, discord, users }
│   ├── discord.rs   # traits Responder (reply to this interaction) + DiscordApi (DMs, lookups), serenity impls
│   ├── users.rs     # trait UserRepo + SqliteUserRepo: the shared `users` table
│   └── error.rs     # FeatureError::User (shown to the user) / Internal (logged, generic reply)
└── features/
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

Conventions:
- Feature code never touches serenity's `Context`/`Http` or a `SqlitePool` directly; it goes through `InteractionCtx` and its own repository trait, so every handler is testable with mocks (`MockResponder`, `MockDiscordApi`, `MockUserRepo`, `MockSecretSantaRepo`). Put `#[cfg_attr(test, mockall::automock)]` above `#[async_trait]`.
- Repository methods are shaped around use cases; writes that belong together run in one transaction, and status changes are conditional (`... WHERE status = ?`) so double clicks are harmless.
- Tables: `framework/` owns unprefixed tables (`users`); each feature owns tables with its own prefix (`ss_*`) and may reference `users(id)`, never another feature's tables. Write any user id to a feature table only after `ctx.users.ensure(..)`.
- The bot requests no gateway intents; interactions don't need them.

### Adding a feature

1. Create `src/features/<name>/` (or `<name>.rs` if small) with a struct implementing `Feature`, and pick a unique `namespace()`; every custom id it emits starts with `<namespace>:`.
2. If it needs storage, define a `<Name>Repo` trait with `#[cfg_attr(test, mockall::automock)]` and a sqlx impl, and add a migration for `<prefix>_*` tables that reference `users(id)`.
3. Add one `.register(...)` line in `main.rs`. `build()` fails at startup on a duplicate command name or namespace.
4. Test handlers with the mocks (`crate::framework::testing` has helpers for building a context and reading responses).

Message events aren't supported yet; `REFACTOR_PLAN.md` ("Adding Message Support Later") describes how to add them.
