#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Fills a data folder with nine days of made-up run history and app time, so the statistics
page has something to show. Everything is generated; nothing here is real usage.

usage: seed.py <data folder>
"""
import datetime
import json
import os
import random
import sys
import time

home = sys.argv[1]
os.makedirs(home, exist_ok=True)
random.seed(7)
now = int(time.time())

APPS = {
    "com.apple.Terminal": "Terminal",
    "com.google.Chrome": "Google Chrome",
    "com.tinyspeck.slackmacgap": "Slack",
    "md.obsidian": "Obsidian",
    "com.apple.mail": "Mail",
    "com.spotify.client": "Spotify",
    "com.microsoft.VSCode": "Code",
    "com.apple.Notes": "Notes",
    "com.figma.Desktop": "Figma",
}
AWAY = ["com.google.Chrome", "com.google.Chrome", "com.tinyspeck.slackmacgap", "md.obsidian",
        "com.apple.mail", "com.figma.Desktop"]
RUNS_PER_DAY = [9, 6, 11, 0, 4, 7, 3, 5, 8]
DURATIONS = [20, 35, 48, 75, 120, 180, 260, 420, 600, 840, 1260]

slots = {}
runs = []


def slot_at(unix):
    """The half-hour slot a moment falls in, as usage.json keys it."""
    return slots.setdefault(str(unix // 1800), {
        "switches": 0, "reactions": 0, "reaction_ms": 0,
        "claude_home_ms": 0, "claude_away_ms": 0, "apps": {},
    })


def add_time(slot, app, ms, while_claude):
    entry = slot["apps"].setdefault(app, {"ms": 0, "claude_ms": 0})
    entry["ms"] += ms
    if while_claude:
        entry["claude_ms"] += ms


for days_ago, count in enumerate(RUNS_PER_DAY):
    for n in range(count):
        start = min(now - days_ago * 86400 - random.randint(600, 9 * 3600), now - 600)
        duration = random.choice(DURATIONS)
        stamp = datetime.datetime.fromtimestamp(start, datetime.timezone.utc)
        runs.append({
            "v": 1, "run_id": f"p{days_ago}-{n}", "project": "0123456789abcdef",
            "start": stamp.strftime("%Y-%m-%dT%H:%M:%SZ"), "duration_s": duration,
            "outcome": "done", "permission_mode": "default", "effort": None,
            "prompt_len": "short", "tools": 3, "subagents": 0, "slow_commands": 0,
            "needed_input": random.random() < 0.3, "predicted_tier": "unknown",
            "away": "none", "return": "prompt", "snoozes": 0,
        })
        slot = slot_at(start)
        away_ms = int(duration * 1000 * random.uniform(0.5, 0.9))
        home_ms = duration * 1000 - away_ms
        add_time(slot, random.choice(AWAY), away_ms, True)
        add_time(slot, "com.apple.Terminal", home_ms, True)
        slot["claude_away_ms"] += away_ms
        slot["claude_home_ms"] += home_ms
        slot["switches"] += random.randint(2, 9)
        slot["reactions"] += 1
        slot["reaction_ms"] += random.randint(4000, 26000)
        for app in random.sample(list(APPS), 3):
            add_time(slot, app, random.randint(60, 900) * 1000, False)

runs.sort(key=lambda run: run["start"])
with open(os.path.join(home, "runs.jsonl"), "w") as out:
    out.write("".join(json.dumps(run) + "\n" for run in runs))
with open(os.path.join(home, "usage.json"), "w") as out:
    json.dump({"v": 1, "names": APPS, "slots": slots}, out)
with open(os.path.join(home, "config.toml"), "w") as out:
    out.write('# my settings\n[receiver]\ntoken = "dev-token"\n')
print(f"seeded {len(runs)} runs and {len(slots)} half-hour slots into {home}")
