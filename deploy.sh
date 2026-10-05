#!/usr/bin/env bash
# Pull the latest code, rebuild the image and restart the bot under Docker Compose.
#
# Assumes it runs on the server in the deployment checkout, with .env, data/ and
# secrets/discord_token in place (README, "Deployment"), as a user who can run docker. The bot
# applies database migrations itself when it starts.
set -euo pipefail
cd "$(dirname "$0")"

echo "==> Pulling"
git pull --ff-only

echo "==> Building and starting"
export JABOT_UID=$(id -u jabot)
export JABOT_GID=$(id -g jabot)
docker compose up -d --build

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
