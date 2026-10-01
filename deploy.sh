#!/usr/bin/env bash
# Build a release binary and install it over the one the systemd service runs.
#
# Assumes it runs on the host that runs the service, from a checkout whose .env lets sqlx check
# queries at compile time, as a user who can sudo. Set JABOT_SERVICE to use another unit name.
set -euo pipefail

service="${JABOT_SERVICE:-jabot}"
cd "$(dirname "$0")"

sudo=""
if [[ $EUID -ne 0 ]]; then
    sudo="sudo"
fi

echo "==> Building release"
cargo build --release
binary="${CARGO_TARGET_DIR:-target}/release/jabot"

# ExecStart looks like `{ path=/opt/jabot/jabot ; argv[]=/opt/jabot/jabot ; ... }`.
# Read it before stopping so a bad unit name doesn't leave the bot down.
exec_start="$(systemctl show --property=ExecStart --value "$service")"
target="$(sed -n 's/.*path=\([^ ;]*\).*/\1/p' <<<"$exec_start")"
if [[ -z "$target" ]]; then
    echo "Could not find the executable path in ExecStart of $service: '$exec_start'" >&2
    exit 1
fi

echo "==> Stopping $service"
$sudo systemctl stop "$service"
# If the install fails, bring the old binary back up rather than leaving the bot down.
trap 'echo "Deploy failed; starting $service again" >&2; $sudo systemctl start "$service"' ERR

echo "==> Installing $binary to $target"
$sudo install -m 0755 "$binary" "$target"

trap - ERR
echo "==> Starting $service"
$sudo systemctl start "$service"
# Type=simple reports success as soon as the process spawns; give a startup panic time to show.
sleep 3
systemctl --no-pager status "$service"
