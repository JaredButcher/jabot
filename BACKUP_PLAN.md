# JABot Plan: Database Backups

**Status:** planned, not started.

## Goals

1. **Daily off-site backups** of the SQLite database to Backblaze B2, encrypted and
   compressed by restic.
2. **The bot runs them.** A timer inside the bot takes a consistent copy with `VACUUM INTO`,
   then runs `restic` as a child process to upload it and prune old snapshots.
3. **Pre-deploy copies.** The deploy script keeps a local copy of the database from just
   before each deploy, so a bad migration can be rolled back in a minute.
4. **Failures are noticed.** A failed backup DMs the bot owner.
5. **Restores are manual and documented.** Nothing restores automatically.

## Decisions

| Question | Decision | Consequence |
|---|---|---|
| Where | One private B2 bucket, accessed through B2's **S3-compatible API** | restic's docs recommend it over the native `b2:` backend, which can hang on errors. The bucket needs the lifecycle rule "Keep only the last version of the file", because restic's S3 backend only hides deleted files on B2 |
| Tool | **restic**, from Debian trixie's `apt` (0.18) in the runtime image | Encryption is built in, and compression is on by default for repository format 2. `RESTIC_COMPRESSION=max` costs nothing at this size. The version moves with the base image |
| Who runs it | **The bot**: a tokio task started from `main.rs`, independent of the Discord client and the HTTP server | It isn't a `Feature` (no commands or interactions), so it lives in `src/backup/`. If the task fails, it logs and keeps going; it never takes the bot down |
| Consistent copy | `VACUUM INTO '/data/backup/database.sqlite'` through the bot's pool, then `PRAGMA integrity_check` on the copy | Safe while the bot is running, with the default rollback journal. Writes wait for a moment, which is negligible at this size |
| When | Daily at `BACKUP_TIME_UTC` (default `08:00`, i.e. 4 a.m. Eastern in summer). At startup, if the latest off-site snapshot is more than 25 hours old, run 2 minutes after startup | Restarts and deploys don't cause missed days. "Latest snapshot" comes from `restic snapshots`, so the bot keeps no backup state of its own |
| Retries | A failed run retries after 1 hour, up to 3 times, then waits for the next day | Covers a B2 or network blip without hammering anything |
| Retention | `restic forget --keep-daily 7 --keep-weekly 4 --keep-monthly 6 --prune` after each successful backup | **This is also how long deleted data survives.** A `/k` value or `tell` token deleted today is still in backups for up to about 6 months. Short enough to be reasonable, long enough to notice slow corruption |
| Checking | `restic check --read-data` once a week (Sundays) | The repository is a few MB, so reading all of it stays well inside B2's free 1 GB a day of downloads |
| Snapshot identity | Always back up the same path, with `--host jabot` and `--tag scheduled` | The container's hostname changes on every recreate. A fixed host keeps snapshots in one series, so restic can reuse the previous snapshot and `forget` applies to all of them |
| Credentials | Docker secrets, like the Discord token, readable by the `jabot` user: `secrets/restic_password` (restic's `RESTIC_PASSWORD_FILE`) and `secrets/b2_credentials`, an AWS-style credentials file (`AWS_SHARED_CREDENTIALS_FILE`) | Nothing secret in `compose.yaml`, `.env` or the process environment. The B2 key is limited to this one bucket |
| Repository setup | `restic init` is run **once, by hand**; the bot never creates a repository | A typo in the repository URL fails loudly, instead of quietly starting a new, empty repository |
| Failure alerts | DM to `BOT_OWNER_ID` on the first failure in a streak, and again when backups work again | One message per incident, not one per retry. With `BOT_OWNER_ID` unset, failures are only logged |
| Startup check | `restic snapshots` at startup, which needs the URL, the key and the password to work, and also gives the latest snapshot's time | A broken setup is reported (log plus DM) at startup, not at 08:00 the next day, and retried like a failed run. The bot keeps running either way |
| Off by default | Backups run only when `RESTIC_REPOSITORY` is set | Local development needs no B2 account; the startup log says backups are disabled, like `HTTP_PORT` |
| Manual run | `jabot backup` (an argument to the same binary) runs one backup and exits with its result | For checking the setup after a deploy: `docker compose exec jabot jabot backup`. No Discord connection; it prints to the terminal |
| Pre-deploy copies | Local only, outside `data/`; see [Deploy backups](#deploy-backups) | They're for quick rollbacks. The daily backup is the off-site copy |

## How a Backup Runs

```
timer fires (or `jabot backup`)
  1. rm -f /data/backup/database.sqlite          VACUUM INTO refuses an existing file
  2. VACUUM INTO '/data/backup/database.sqlite'  through the shared pool
  3. PRAGMA integrity_check on the copy          separate read-only connection; anything but "ok" fails the run
  4. restic unlock                               clears a stale lock from a run killed mid-way (e.g. by a deploy)
  5. restic backup /data/backup/database.sqlite --host jabot --tag scheduled
  6. restic forget --host jabot --tag scheduled --keep-daily 7 --keep-weekly 4 --keep-monthly 6 --prune
  7. Sundays: restic check --read-data
  8. rm /data/backup/database.sqlite             the database is the only plaintext copy left
  → success: log the snapshot id and size; DM "working again" if the last run failed
  → failure: log restic's stderr; DM the owner if this starts a failure streak; schedule a retry
```

- **Timeouts.** Each restic call gets 10 minutes, and runs with `kill_on_drop`, so a hung
  network can't stall the task, and shutting down the bot doesn't leave restic running.
- **Environment.** restic reads its own variables (`RESTIC_REPOSITORY`,
  `RESTIC_PASSWORD_FILE`, `AWS_SHARED_CREDENTIALS_FILE`, `RESTIC_COMPRESSION`,
  `RESTIC_CACHE_DIR`), and the child inherits them from the bot. The bot only reads
  `RESTIC_REPOSITORY`, to decide whether backups are on.
- **Cache.** `RESTIC_CACHE_DIR=/data/.cache/restic`, so it survives container recreates and
  belongs to `jabot`. It has to be explicit: the container runs as the host's `jabot` uid,
  which has no home directory inside the image.

## Code Layout

```
src/backup/
├── mod.rs        # BackupConfig::from_env, run_scheduler(config, pool, notifier), run_once
├── schedule.rs   # pure: next run from unix time, BACKUP_TIME_UTC, last snapshot time, retry state
├── snapshot.rs   # VACUUM INTO + integrity_check (sqlx tests against a real database)
├── restic.rs     # trait Restic (automock) + ResticCli: tokio::process, timeouts, --json parsing
└── run.rs        # one run: snapshot → restic → cleanup; BackupError; failure-streak tracking
```

```rust
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait Restic: Send + Sync {
    /// Time of the newest `scheduled` snapshot, if any. Fails if the repository, the key or
    /// the password is wrong, so it doubles as the startup check.
    async fn latest_snapshot(&self) -> Result<Option<SystemTime>, ResticError>;
    /// Remove stale locks left by a run that was killed.
    async fn unlock(&self) -> Result<(), ResticError>;
    async fn backup(&self, path: &Path) -> Result<SnapshotSummary, ResticError>;
    async fn forget_and_prune(&self) -> Result<(), ResticError>;
    async fn check(&self) -> Result<(), ResticError>;
}
```

- **`ResticError`** carries the exit status and the last lines of stderr, so a DM can say
  "Fatal: wrong password" rather than "exit status 1".
- **Alerts** go through the existing `DiscordApi::send_dm`, with `MockDiscordApi` in tests.
- **`schedule.rs`** works on unix seconds in UTC, so it needs no date crate and no timezone
  handling.
- **In `main.rs`:** after building the client, `tokio::spawn(backup::run_scheduler(..))` with
  the pool and a `SerenityDiscordApi`. It stays out of the `select!`, so the backup task ending
  never ends the process. `jabot backup` takes a separate path before the Discord client is
  created.

## Configuration

`compose.yaml` additions:

```yaml
    environment:
      RESTIC_REPOSITORY: ${RESTIC_REPOSITORY:-}
      RESTIC_PASSWORD_FILE: /run/secrets/restic_password
      AWS_SHARED_CREDENTIALS_FILE: /run/secrets/b2_credentials
      RESTIC_COMPRESSION: max
      RESTIC_CACHE_DIR: /data/.cache/restic
      BACKUP_TIME_UTC: ${BACKUP_TIME_UTC:-08:00}
      BOT_OWNER_ID: ${BOT_OWNER_ID:-}
    secrets:
      - discord_token
      - restic_password
      - b2_credentials

secrets:
  restic_password:
    file: ./secrets/restic_password
  b2_credentials:
    file: ./secrets/b2_credentials
```

`.env.example` gains `RESTIC_REPOSITORY`, `BACKUP_TIME_UTC` and `BOT_OWNER_ID`, e.g.
`RESTIC_REPOSITORY=s3:https://s3.us-west-004.backblazeb2.com/jabot-backups/jabot`.

`secrets/b2_credentials`:

```ini
[default]
aws_access_key_id = <B2 keyID>
aws_secret_access_key = <B2 applicationKey>
```

## One-Time Setup

1. **B2:**
   1. Create a private bucket, e.g. `jabot-backups`.
   2. Set the lifecycle rule to "Keep only the last version of the file".
   3. Create an application key with read and write access to **that bucket only**. `forget
      --prune` needs to delete, so write access is required.
   4. Note the bucket's S3 endpoint, e.g. `s3.us-west-004.backblazeb2.com`.
2. **Secrets on the Pi:**
   1. `openssl rand -base64 32 > secrets/restic_password`.
   2. Write `secrets/b2_credentials`.
   3. `sudo chown jabot:jabot` both files and `chmod 600` them. The container runs as the host's
      `jabot` user (`JABOT_UID`/`JABOT_GID`, which `deploy.sh` exports).
3. **Store the restic password and the B2 key in your password manager.** Without the
   password, the backups can't be decrypted by anyone, including you.
4. **Create the repository:** `docker compose run --rm --entrypoint restic jabot init`. Like
   every `docker compose` command run by hand here, this needs
   `export JABOT_UID=$(id -u jabot) JABOT_GID=$(id -g jabot)` in the shell first.
5. **Deploy, then check:** `docker compose exec jabot jabot backup`, then
   `docker compose exec jabot restic snapshots`.

## Restoring

From the Pi, or any machine with restic and the two secrets:

```sh
docker compose run --rm --entrypoint restic jabot snapshots --tag scheduled
mkdir restore && sudo chown jabot:jabot restore    # the container writes as jabot
docker compose run --rm --entrypoint restic -v "$PWD/restore:/restore" jabot \
    restore latest --tag scheduled --target /restore
docker compose stop jabot
sudo cp restore/data/backup/database.sqlite data/database.sqlite
sudo chown jabot:jabot data/database.sqlite
docker compose start jabot
```

- **The code must be at least as new as the backup.** sqlx refuses to start if the database
  has a migration the code doesn't know. An older backup is fine: it migrates forward at
  startup.
- **Practise a restore once,** to a scratch directory, after setup, and again after any change
  to the backup code.

## Deploy Backups

`deploy.sh` (the Docker deploy; the systemd one has been removed) copies the database just
before the new version starts. Recommendations:

1. **Build first, then stop, copy, start.**
   1. Record the running commit (`git rev-parse --short HEAD`) **before** `git pull`.
   2. `git pull --ff-only`.
   3. `docker compose build`, while the old bot keeps running. A failed build changes nothing.
   4. `docker compose stop jabot`.
   5. Copy the database.
   6. `docker compose up -d`.

   Downtime grows only by the copy (milliseconds), because `up` recreates the container
   anyway.
2. **Copy with the bot stopped, using plain `cp`.** After a clean stop, the file is complete
   and consistent, and the copy is byte-for-byte what the old version left, before any new
   migration touches it. No `sqlite3` tool is needed on the host.
   - If `data/database.sqlite-journal` exists (the last shutdown wasn't clean), copy it next to
     the copy under the matching name. SQLite rolls it back when the copy is opened.
   - On the first deploy there's no database yet; skip the copy.
3. **Keep the copies outside `data/`**, in `./backups/deploy/` next to `compose.yaml`.
   - It isn't mounted into the container, so neither the bot nor a bug in it can delete the
     copies.
   - It belongs to the deploy user, who must be able to read `data/database.sqlite` (owned by
     `jabot`; SQLite creates it `0644`).
   - Use `chmod 700` on the directory and `install -m 600` for each copy: they contain `tell`
     tokens and `/k` values.
   - Add `backups/` to `.gitignore` and `.dockerignore`. The `Dockerfile`'s `COPY . .` would
     otherwise send them to the build.
4. **Name copies by time and the commit that wrote them:**
   `backups/deploy/20261005T143000Z-ba90a9a.sqlite`. The commit is the code that can open the
   copy, which is exactly what a rollback needs.
5. **Keep the 10 newest** (`DEPLOY_BACKUPS_KEEP`, default 10), deleting older ones at the end
   of a successful deploy. They're tiny; the limit is about not hoarding personal data.
6. **If anything fails after the stop, start the old container again** (a `trap` on `ERR`),
   so a failed copy never leaves the bot down.
7. **Keep them local.** The daily backup is the off-site copy. If a deploy copy should also
   go off-site, `docker compose exec jabot jabot backup` after the deploy does it.
8. **Document rolling back a bad deploy** in the README:

   ```sh
   docker compose stop jabot
   git checkout <commit from the backup's name>
   sudo install -o jabot -g jabot -m 644 backups/deploy/<file>.sqlite data/database.sqlite
   docker compose up -d --build
   ```

   Rolling back the code without the database fails at startup once a newer migration has
   run, so the two always go together.

## Testing

- **schedule.rs** (pure, unix seconds):
  - The next run is today or tomorrow at `BACKUP_TIME_UTC`.
  - Catch-up when the latest snapshot is over 25 hours old, or missing.
  - No catch-up right after a recent backup.
  - Retries at +1 hour, at most 3, then the next day.
  - `BACKUP_TIME_UTC` parsing (`08:00`, `8:00`, bad values refused at startup).
- **snapshot.rs** (`#[sqlx::test]`):
  - `VACUUM INTO` produces a database with the same rows.
  - An existing target file is replaced.
  - A corrupted copy fails `integrity_check`, tested by overwriting bytes in the copy.
- **run.rs** (`MockRestic`, `MockDiscordApi`, a real pool):
  - Order: snapshot, `backup`, `forget_and_prune`, cleanup. `check` runs only on Sundays.
  - A failing `backup` skips `forget`, deletes the copy and reports.
  - The first failure DMs the owner, repeat failures don't, and the first success after them
    DMs "working again".
  - No DMs without `BOT_OWNER_ID`.
- **restic.rs:**
  - Parsing `restic snapshots --json` and `backup --json` output (fixtures).
  - Building the command lines.
  - A timeout kills the child (tested with `sleep` standing in for restic).
- **Manual:**
  - The one-time setup against a real bucket, then `jabot backup`, `restic snapshots`, a
    restore to a scratch directory, and `PRAGMA integrity_check` on the result.
  - A wrong password: the startup check logs and DMs.
  - The B2 key deleted: the daily run fails, DMs once, retries, and DMs "working again" once
    a new key is in place.
  - `deploy.sh` twice: two copies in `backups/deploy/`, named by commit, and the 11th deploy
    prunes the oldest.

## Steps

Each step compiles, passes `cargo test` and `cargo clippy --all-targets`, and gets its own
commit.

1. **Snapshot.** `src/backup/snapshot.rs` with `VACUUM INTO` and the integrity check.
2. **restic wrapper.** The `Restic` trait and `ResticCli`, with timeouts and output parsing.
3. **One run.** `run.rs`, with failure-streak alerts and `BOT_OWNER_ID`.
4. **Scheduler.** `schedule.rs` plus `run_scheduler`, the startup check, and wiring in
   `main.rs`; also `jabot backup`.
5. **Image and compose.** Install `restic` in the `Dockerfile`; add the secrets and env to
   `compose.yaml` and `.env.example`.
6. **Deploy copies.** `deploy.sh`, plus `backups/` in `.gitignore` and `.dockerignore`.
7. **Docs.**
   - README: setup, restore, rollback.
   - CLAUDE.md: `src/backup/` in the architecture, the new env vars, and that deploys keep
     copies in `backups/deploy/`.
8. **Manual test on the Pi**, including a restore.

## Settled Questions

- **Retention:** 7 daily, 4 weekly and 6 monthly snapshots, so deleted data lingers in backups
  for up to about 6 months.
- **`BOT_OWNER_ID`** names the bot's admin generally, so later admin-only commands can reuse
  it.
- **Deploys:** only the Docker deploy (`deploy.sh`) is in use.

## Deferred

- **A check-in monitor** (e.g. healthchecks.io) that alerts when backups *stop*, including
  when the bot itself is down and can't DM anyone. Needs an HTTP client or a `curl` child
  process.
- **`/backup` command** for the owner: run now, show the latest snapshot.
- **Backups the bot can't delete**, using B2 Object Lock or a key without delete rights,
  pruned by a separate admin key. Protects against a compromised Pi wiping its own backups,
  at the cost of a second key and manual pruning.
- **Hashing `tell` tokens** in the database, so a leaked backup doesn't expose working
  tokens. Tokens are stored in plain text today because `/tell` shows them again.
