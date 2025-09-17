# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

JABot is a Rust Discord bot with features such as orchestrating a secret santa. It uses the Serenity Discord API library and SQLx for database operations with SQLite.

## Database Setup

SQLx requires database access at compile time for query validation. Before building:

1. Install SQLx CLI: https://github.com/launchbadge/sqlx/tree/main/sqlx-cli
2. Create `.env` file with: `DATABASE_URL=sqlite:database.sqlite`
3. Run `sqlx database setup` to create the database and apply migrations from `migrations/`

## Development Commands

- **Build**: `cargo build`
- **Run**: `cargo run` (requires `DISCORD_TOKEN_FILE` environment variable pointing to file with Discord API token)
- **Run with explicit env**: `DATABASE_URL=sqlite:database.sqlite DISCORD_TOKEN_FILE=... cargo run`
- **Test**: `cargo test`

## Environment Variables

- `DATABASE_URL`: Required for SQLx, typically `sqlite:database.sqlite`
- `DISCORD_TOKEN_FILE`: Path to file containing Discord API token

## Dependencies

- `serenity = "0.12"` - Discord API library
- `sqlx = { version = "0.7", features = ["runtime-tokio-rustls", "sqlite"] }` - Database toolkit

## Architecture

This is a minimal Discord bot project currently containing only a basic "Hello, world!" main function. The architecture is designed to use:
- Serenity for Discord API interactions
- SQLx for compile-time checked database queries with SQLite
- Migration-based database schema management