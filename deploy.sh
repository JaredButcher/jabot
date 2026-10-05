#!/usr/bin/env bash
# Pull the latest code, rebuild the image and restart the bot under Docker Compose. Before the
# new version starts (and runs any new migrations), the database is copied to backups/deploy/,
# named after the time and the commit that wrote it; the newest DEPLOY_BACKUPS_KEEP (default 10)
# are kept. README, "Rolling back a deploy", shows how to use them.
#
# Assumes it runs on the server in the deployment checkout, with .env, data/ and the secrets in
# place (README, "Deployment"), as a user who can run docker and read data/database.sqlite.
# The bot applies database migrations itself when it starts.
set -euo pipefail
cd "$(dirname "$0")"

# The container runs as the host's jabot user (compose.yaml). Assigned before export, so a
# missing user stops the deploy instead of passing an empty id.
JABOT_UID="$(id -u jabot)"
JABOT_GID="$(id -g jabot)"
export JABOT_UID JABOT_GID

keep="${DEPLOY_BACKUPS_KEEP:-10}"
database=data/database.sqlite
copies=backups/deploy

# The commit running now wrote the database, so it's the code that can open the copy.
running="$(git rev-parse --short HEAD)"

echo "==> Pulling"
git pull --ff-only

# Build while the old bot keeps running: a failed build changes nothing.
echo "==> Building"
docker compose build

if [[ -f "$database" ]]; then
    echo "==> Stopping jabot to copy the database"
    docker compose stop jabot
    # From here until the new container starts, a failure mustn't leave the bot down.
    trap 'echo "Deploy failed; starting the old container again" >&2; docker compose start jabot' ERR

    # The copies hold tell tokens and /k values: readable by this user only.
    mkdir -p "$copies"
    chmod 700 backups "$copies"
    copy="$copies/$(date -u +%Y%m%dT%H%M%SZ)-$running.sqlite"
    # After a clean stop the file is complete; plain cp is an exact, consistent copy.
    install -m 600 "$database" "$copy"
    # A journal left by an unclean shutdown belongs with the copy; SQLite rolls it back on open.
    if [[ -f "$database-journal" ]]; then
        install -m 600 "$database-journal" "$copy-journal"
    fi
    echo "==> Saved $copy"
fi

echo "==> Starting"
docker compose up -d
trap - ERR

# A startup failure (e.g. a database that can't be opened) shows within seconds. The restart
# policy would retry it in a loop, so check the state instead of trusting `up`.
sleep 5
state="$(docker compose ps --all --format '{{.State}}' jabot)"
docker compose logs --tail 20 jabot
if [[ "$state" != "running" ]]; then
    echo "jabot is $state, not running; see the logs above" >&2
    exit 1
fi
echo "==> jabot is running"

# Prune only after a successful deploy, so a failed one keeps every copy. Names start with
# the UTC time, so the glob's sorted order is oldest first.
shopt -s nullglob
saved=("$copies"/*.sqlite)
if (( ${#saved[@]} > keep )); then
    for old in "${saved[@]:0:${#saved[@]}-keep}"; do
        rm -f -- "$old" "$old-journal"
        echo "==> Removed old copy $old"
    done
fi
