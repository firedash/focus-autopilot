#!/bin/sh
# SPDX-License-Identifier: Apache-2.0
# Checks the statistics and configuration page in a real browser: builds focus-headless, seeds
# a throwaway data folder, starts the app on it, and runs check.js against the page.
#
# Needs Python 3, Node.js and Playwright with Chromium (`npm i playwright` or a global install).
#
# usage: scripts/ui-check/run.sh [folder for screenshots]
set -eu

here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
port=${FOCUS_UI_PORT:-47899}
work=$(mktemp -d)
shots=${1:-}

cleanup() {
  [ -n "${app_pid:-}" ] && kill "$app_pid" 2>/dev/null || true
  [ -n "${feed_pid:-}" ] && kill "$feed_pid" 2>/dev/null || true
}
trap cleanup EXIT

(cd "$repo" && cargo build --quiet -p focus-headless)
python3 "$here/seed.py" "$work/home"

# The app reads commands from stdin and stops when it closes, so keep a pipe open to it.
mkfifo "$work/stdin"
sleep 600 > "$work/stdin" &
feed_pid=$!
FOCUS_AUTOPILOT_HOME="$work/home" "$repo/target/debug/focus-headless" \
  --port "$port" --home-app com.apple.Terminal --away-app com.apple.Notes \
  < "$work/stdin" > "$work/app.log" 2>&1 &
app_pid=$!
sleep 1

[ -n "$shots" ] && mkdir -p "$shots"
FOCUS_UI_PORT="$port" node "$here/check.js" "$work/home" "$work/app.log" $shots
