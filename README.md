# JABot
JABot
A rust discord bot with features such as orchestrating a secret santa and storing text under keys

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
long_task; curl -fsS https://example.com/jabot/tell \
     -H 'Content-Type: application/json' \
     -d '{"token": "tell_...", "message": "long_task finished"}'
```

The commands avoid newer curl options (`--json` needs 7.82, `--fail-with-body` 7.76). With `-f`, a failed request makes curl exit non-zero and print only the HTTP status; drop `-f` to see the JSON error instead.

Responses are JSON. `204` means the DM was sent. Errors look like `{"error": "..."}`: `401` for a bad token, `400`/`413` for a missing or over-2000-character message, `422` if Discord won't let the bot DM you, and `429` (with `Retry-After`) when rate limited. Each user may send bursts of 5, then 5 a minute.

The HTTP server only runs when `HTTP_PORT` is set. Locally, `HTTP_PORT=8080 cargo run` serves `http://localhost:8080/jabot/tell`.

# k
A personal key-value store. Keys are per user, case-insensitive, and up to 64 characters.

- `/k set key:<key>` DMs you a prompt (and the key's current value, if it has one). Your next DM to the bot within 10 minutes becomes the value, exactly as typed, Markdown included, up to 2000 characters. Only text is stored, not attachments. The prompt has **Cancel**, and **Delete** when the key already exists.
- `/k get key:<key>` finds keys containing what you typed. An exact match, or the only match, shows its value; several matches are listed, 25 per page.
- `/k get` without a key lists all your keys.
- Both commands suggest your keys as you type.

Replies are only visible to you, except in the bot's DM, where they're normal messages. The bot needs the `DIRECT_MESSAGES` gateway intent, which isn't privileged, so nothing has to change in the Developer Portal.

# Deployment
The bot runs under Docker Compose and serves plain HTTP. A front proxy, shared with other web servers on the domain, terminates TLS and forwards `/jabot/*` to it unchanged.

On the server, in a checkout of this repository:

1. `cp .env.example .env` and fill it in (`DOMAIN`, `TRUSTED_PROXIES`, `RESTIC_REPOSITORY`, `BOT_OWNER_ID`, ...).
2. Put the Discord token in `secrets/discord_token`, and the database (if moving an existing one) at `data/database.sqlite`.
3. Set up backups (see "Backups"). At the least, create `secrets/restic_password` and `secrets/b2_credentials`; compose refuses to start without them, but they may be empty while `RESTIC_REPOSITORY` is.
4. `sudo chown -R jabot:jabot data secrets` and `chmod 600 secrets/*`. The container runs as the host's `jabot` user (`deploy.sh` exports its `JABOT_UID`/`JABOT_GID`), which must be able to write `data/`.
5. `./deploy.sh`. It needs to read `data/database.sqlite`, to copy it before each deploy.

For any other `docker compose` command run by hand, `export JABOT_UID=$(id -u jabot) JABOT_GID=$(id -g jabot)` first.

The bot's port is published on `127.0.0.1:8080` by default (`HTTP_PUBLISH`, `HTTP_PORT`).

The front proxy must:
- terminate TLS for `DOMAIN` (and for `LAN_HOST`, if set);
- forward `/jabot/*` to the bot without rewriting the path;
- append the client address to `X-Forwarded-For`.

Its address as seen by the bot goes in `TRUSTED_PROXIES`. A proxy on the same machine connects through the published port from `172.30.0.1`. With `RUST_LOG=jabot=debug`, the bot logs each request's peer address.

If the proxy runs on another machine, tokens and messages cross the network unencrypted. Encrypt that link (TLS or a VPN) first.

# Backups
Daily at `BACKUP_TIME_UTC` (default 08:00 UTC), the bot copies its database with `VACUUM INTO` and uploads it with [restic](https://restic.net), encrypted and compressed, to a Backblaze B2 bucket. It keeps 7 daily, 4 weekly and 6 monthly snapshots, and verifies the whole repository on Sundays. If the bot was down at that time, it catches up shortly after starting. A failed backup is retried hourly, up to 3 times, and `BOT_OWNER_ID` gets a DM when backups start failing and when they work again.

Deleted data (a `/k` value, a revoked `tell` token) stays in backups until its snapshots expire, up to about 6 months.

## Setting up
1. In B2, create a private bucket (e.g. `jabot-backups`) and set its lifecycle rule to "Keep only the last version of the file". restic uses B2's S3 API, which only hides deleted files.
2. Create an application key with read and write access to that bucket only, and note the bucket's S3 endpoint (e.g. `s3.us-west-004.backblazeb2.com`).
3. On the server:
   ```sh
   openssl rand -base64 32 > secrets/restic_password
   cat > secrets/b2_credentials <<'EOF'
   [default]
   aws_access_key_id = <keyID>
   aws_secret_access_key = <applicationKey>
   EOF
   sudo chown jabot:jabot secrets/restic_password secrets/b2_credentials
   chmod 600 secrets/restic_password secrets/b2_credentials
   ```
4. **Store the restic password and the B2 key in a password manager.** Without the password, nobody can decrypt the backups, including you.
5. In `.env`, set `RESTIC_REPOSITORY=s3:https://<endpoint>/<bucket>/jabot`, and `BOT_OWNER_ID` to your Discord user id.
6. Create the repository, deploy, and check:
   ```sh
   export JABOT_UID=$(id -u jabot) JABOT_GID=$(id -g jabot)
   docker compose run --rm --entrypoint restic jabot init
   ./deploy.sh
   docker compose exec jabot jabot backup     # one backup now
   docker compose exec jabot restic snapshots
   ```

## Restoring
The bot never restores by itself.

```sh
export JABOT_UID=$(id -u jabot) JABOT_GID=$(id -g jabot)
docker compose run --rm --entrypoint restic jabot snapshots --tag scheduled
mkdir restore && sudo chown jabot:jabot restore
docker compose run --rm --entrypoint restic -v "$PWD/restore:/restore" jabot \
    restore latest --tag scheduled --target /restore      # or a snapshot id instead of latest
docker compose stop jabot
sudo install -o jabot -g jabot -m 644 restore/data/backup/database.sqlite data/database.sqlite
docker compose start jabot
```

The code must be at least as new as the backup: the bot refuses to start on a database with a migration it doesn't know. Older backups are fine; they migrate forward at startup. Try a restore into a scratch directory once after setting up, so the steps are known to work.

## Rolling back a deploy
`deploy.sh` copies the database to `backups/deploy/<UTC time>-<commit>.sqlite` just before the new version starts, and keeps the newest 10 (`DEPLOY_BACKUPS_KEEP`). The commit in the name is the code that wrote it. To undo a deploy whose migration or code went wrong:

```sh
export JABOT_UID=$(id -u jabot) JABOT_GID=$(id -g jabot)
docker compose stop jabot
git checkout <commit from the copy's name>
sudo install -o jabot -g jabot -m 644 backups/deploy/<copy>.sqlite data/database.sqlite
docker compose up -d --build
```

Roll back the code and the database together: after a new migration has run, the old code refuses the new database.
