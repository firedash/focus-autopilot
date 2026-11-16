#!/bin/sh
# SPDX-License-Identifier: Apache-2.0
# Regenerates the screenshots in docs/images from made-up data: builds focus-headless, seeds a
# throwaway data folder, starts the app on it, and takes the pictures with Chromium.
#
# usage: scripts/ui-check/screenshots.sh [folder for the images, default docs/images]
set -eu

here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
port=${FOCUS_UI_PORT:-47898}
work=$(mktemp -d)
images=${1:-$repo/docs/images}

cleanup() {
  [ -n "${app_pid:-}" ] && kill "$app_pid" 2>/dev/null || true
  [ -n "${feed_pid:-}" ] && kill "$feed_pid" 2>/dev/null || true
}
trap cleanup EXIT

(cd "$repo" && cargo build --quiet -p focus-headless)
python3 "$here/seed.py" "$work/home"

mkfifo "$work/stdin"
sleep 300 > "$work/stdin" &
feed_pid=$!
FOCUS_AUTOPILOT_HOME="$work/home" "$repo/target/debug/focus-headless" \
  --port "$port" --home-app com.apple.Terminal --away-app com.apple.Notes \
  < "$work/stdin" > "$work/app.log" 2>&1 &
app_pid=$!
sleep 1

mkdir -p "$images"
FOCUS_UI_PORT="$port" node "$here/screenshots.js" "$images"
