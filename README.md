# JABot
JABot
A rust discord bot with features such as orchestrating a secret santa

https://github.com/serenity-rs/serenity

# Setup
In order to compile the project, a database needs to be set-up. That's because SQLx accesses the database at compile time to make sure your SQL queries are correct.

https://github.com/launchbadge/sqlx/tree/main/sqlx-cli

To set up the database, download the SQLx CLI and run `sqlx database setup`. This command will create the database and its tables by applying the migration files in `migrations/`.

Most SQLx CLI commands require the `DATABASE_URL` environment variable to be set to the database URL, for example `sqlite:database.sqlite` (where `sqlite:` is the protocol and `database.sqlite` the actual filename). A convenient way to supply this information to SQLx is to create a `.env` file which SQLx automatically detects and reads:

```DATABASE_URL=sqlite:database.sqlite```

Expects an envirment variable `DISCORD_TOKEN_FILE` with a parth to a file containing a discord api token.

Build with `cargo build`.

# Run
Can be run with without enviroment files. Though `.env` should be used.

```DATABASE_URL=sqlite:examples/e16_sqlite_database/database.sqlite DISCORD_TOKEN_FILE=... cargo run```

# tell
`/tell` replies (only to you) with a token and a ready-to-run `curl` command. Running it again shows the same token until you press **Revoke token**. Sending a request with the token DMs you the message, e.g. at the end of a long task:

```sh
long_task; curl -sS --fail-with-body https://example.com/jabot/tell \
     --json '{"token": "tell_...", "message": "long_task finished"}'
```

`--json` needs curl 7.82 or newer. On older curl, use `-H 'Content-Type: application/json' -d '...'` instead.

Responses are JSON. `204` means the DM was sent. Errors look like `{"error": "..."}`: `401` for a bad token, `400`/`413` for a missing or over-2000-character message, `422` if Discord won't let the bot DM you, and `429` (with `Retry-After`) when rate limited. Each user may send bursts of 5, then 5 a minute.

The HTTP server only runs when `HTTP_PORT` is set. Locally, `HTTP_PORT=8080 cargo run` serves `http://localhost:8080/jabot/tell`.

# Deployment
The bot runs under Docker Compose and serves plain HTTP. A front proxy, shared with other web servers on the domain, terminates TLS and forwards `/jabot/*` to it unchanged.

On the server, in a checkout of this repository:

1. `cp .env.example .env` and fill it in (`DOMAIN`, `TRUSTED_PROXIES`, ...).
2. Put the Discord token in `secrets/discord_token`, and the database (if moving an existing one) at `data/database.sqlite`.
3. `sudo chown -R 10001:10001 data secrets`. The container runs as uid 10001 and must be able to write `data/`.
4. `./deploy.sh`, or `docker compose up -d --build` the first time.

The bot's port is published on `127.0.0.1:8080` by default (`HTTP_PUBLISH`, `HTTP_PORT`).

The front proxy must:
- terminate TLS for `DOMAIN` (and for `LAN_HOST`, if set);
- forward `/jabot/*` to the bot without rewriting the path;
- append the client address to `X-Forwarded-For`.

Its address as seen by the bot goes in `TRUSTED_PROXIES`. A proxy on the same machine connects through the published port from `172.30.0.1`. With `RUST_LOG=jabot=debug`, the bot logs each request's peer address.

If the proxy runs on another machine, tokens and messages cross the network unencrypted. Encrypt that link (TLS or a VPN) first.
