// SPDX-License-Identifier: Apache-2.0
//! Where screen time goes, for the statistics page: how long each app was in front, whether
//! Claude was working at the time, how often the front app changed, and how quickly the user
//! came back when called. Only app ids and names are kept, never window titles or content,
//! and the file never leaves the machine. Turned off with `stats.track_app_time = false`.
//!
//! The shell looks at the front app twice a second and credits the time since its last look to
//! the app that was in front, under whatever was true at that moment. So each figure is good
//! to about half a second per app switch, which is noise against minutes of use.
//!
//! Time is kept in half-hour slots keyed by `unix time / 1800`, so the statistics page can
//! group them into days in the viewer's own time zone.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write as _;
use std::path::Path;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

pub const SLOT_SECS: u64 = 1_800;
const SECS_PER_DAY: u64 = 86_400;
/// A gap between samples longer than this is not counted: the machine slept or the loop
/// stalled, and nobody was looking at the front app for that long.
const MAX_GAP: Duration = Duration::from_secs(5);
/// With no keyboard or mouse input for this long, the user is taken to be away from the
/// machine and the front app stops accumulating time.
pub const IDLE_CUTOFF: Duration = Duration::from_secs(300);
/// A wait longer than this for the user to come back says they were away, not slow.
const MAX_REACTION: Duration = Duration::from_secs(3_600);
const SAVE_EVERY: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    #[serde(default)]
    pub v: u32,
    /// Display name for each app id seen.
    #[serde(default)]
    pub names: BTreeMap<String, String>,
    #[serde(default)]
    pub slots: BTreeMap<u64, Slot>,
}

/// One half hour. Durations are in milliseconds.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Slot {
    /// Times the front app changed.
    #[serde(default)]
    pub switches: u32,
    /// Times the user came back after being called, and the total wait.
    #[serde(default)]
    pub reactions: u32,
    #[serde(default)]
    pub reaction_ms: u64,
    /// While Claude was working: time in a home app, and time anywhere else.
    #[serde(default)]
    pub claude_home_ms: u64,
    #[serde(default)]
    pub claude_away_ms: u64,
    #[serde(default)]
    pub apps: BTreeMap<String, AppTime>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppTime {
    /// Time in front.
    #[serde(default)]
    pub ms: u64,
    /// The part of it during which Claude was working.
    #[serde(default)]
    pub claude_ms: u64,
}

/// What was true during one sample.
#[derive(Debug, Clone, Copy)]
pub struct Moment<'a> {
    pub app_id: &'a str,
    pub app_name: &'a str,
    pub is_home: bool,
    pub claude_working: bool,
}

impl Usage {
    fn slot(&mut self, unix: u64) -> &mut Slot {
        self.slots.entry(unix / SLOT_SECS).or_default()
    }

    pub fn add_time(&mut self, unix: u64, moment: Moment<'_>, ms: u64) {
        if ms == 0 {
            return;
        }
        if self.names.get(moment.app_id).map(String::as_str) != Some(moment.app_name) {
            self.names
                .insert(moment.app_id.to_owned(), moment.app_name.to_owned());
        }
        let slot = self.slot(unix);
        let app = slot.apps.entry(moment.app_id.to_owned()).or_default();
        app.ms += ms;
        if moment.claude_working {
            app.claude_ms += ms;
            if moment.is_home {
                slot.claude_home_ms += ms;
            } else {
                slot.claude_away_ms += ms;
            }
        }
    }

    pub fn add_switch(&mut self, unix: u64) {
        self.slot(unix).switches += 1;
    }

    pub fn add_reaction(&mut self, unix: u64, waited: Duration) {
        let slot = self.slot(unix);
        slot.reactions += 1;
        slot.reaction_ms += u64::try_from(waited.as_millis()).unwrap_or(u64::MAX);
    }

    /// Drops slots older than `retention_days`, and names no slot uses any more.
    pub fn prune(&mut self, now_unix: u64, retention_days: u32) {
        let oldest = now_unix.saturating_sub(u64::from(retention_days) * SECS_PER_DAY) / SLOT_SECS;
        self.slots.retain(|slot, _| *slot >= oldest);
        let slots = &self.slots;
        self.names
            .retain(|id, _| slots.values().any(|s| s.apps.contains_key(id)));
    }

    /// A missing or unreadable file is an empty history, not an error: statistics are a
    /// convenience and must never stop the app.
    pub fn load(path: &Path) -> Self {
        fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// Writes the file whole, through a temporary file, readable by the owner only.
    ///
    /// # Errors
    /// When the folder or file can't be written.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let dir = path.parent().ok_or("usage path has no folder")?;
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let mut body = self.clone();
        body.v = 1;
        let text = serde_json::to_string(&body).map_err(|e| e.to_string())?;
        let tmp = dir.join(format!(".usage.{}.tmp", std::process::id()));
        let mut options = fs::OpenOptions::new();
        options.create(true).write(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let written = options
            .open(&tmp)
            .and_then(|mut file| file.write_all(text.as_bytes()))
            .and_then(|()| fs::rename(&tmp, path));
        written.map_err(|e| {
            let _ = fs::remove_file(&tmp);
            format!("{}: {e}", path.display())
        })
    }
}

/// Turns the shell loop's observations into [`Usage`] entries.
#[derive(Debug)]
pub struct Tracker {
    last_sample: Instant,
    /// When the user was last called back and hasn't come back yet.
    called_at: Option<Instant>,
    dirty: bool,
    last_save: Instant,
}

impl Tracker {
    pub fn new(now: Instant) -> Self {
        Self {
            last_sample: now,
            called_at: None,
            dirty: false,
            last_save: now,
        }
    }

    /// Credits the time since the last sample to the app in front. `idle` is the time since
    /// the last keyboard or mouse input, if it could be read.
    pub fn sample(
        &mut self,
        usage: &mut Usage,
        now: Instant,
        unix: u64,
        front: Option<Moment<'_>>,
        idle: Option<Duration>,
    ) {
        let gap = now.saturating_duration_since(self.last_sample);
        self.last_sample = now;
        let Some(front) = front else { return };
        if gap > MAX_GAP || idle.is_some_and(|idle| idle >= IDLE_CUTOFF) {
            return;
        }
        let ms = u64::try_from(gap.as_millis()).unwrap_or(0);
        if ms > 0 {
            usage.add_time(unix, front, ms);
            self.dirty = true;
        }
    }

    pub fn switched(&mut self, usage: &mut Usage, unix: u64) {
        usage.add_switch(unix);
        self.dirty = true;
    }

    /// Follows the core's log events to time how long the user takes to come back.
    pub fn log(&mut self, usage: &mut Usage, name: &str, now: Instant, unix: u64) {
        match name {
            "return_prompt" | "return_countdown" => {
                self.called_at.get_or_insert(now);
            }
            "returned" | "came_back" => {
                if let Some(called) = self.called_at.take() {
                    let waited = now.saturating_duration_since(called);
                    if waited <= MAX_REACTION {
                        usage.add_reaction(unix, waited);
                        self.dirty = true;
                    }
                }
            }
            // A new prompt means the call was answered some other way or no longer matters.
            "run_started" => self.called_at = None,
            _ => {}
        }
    }

    /// Forgets unsaved-ness, for after the history was cleared.
    pub fn reset(&mut self) {
        self.called_at = None;
        self.dirty = true;
    }

    /// True when there is something new and the last save was long enough ago.
    pub fn save_due(&self, now: Instant) -> bool {
        self.dirty && now.saturating_duration_since(self.last_save) >= SAVE_EVERY
    }

    pub fn has_unsaved(&self) -> bool {
        self.dirty
    }

    pub fn saved(&mut self, now: Instant) {
        self.dirty = false;
        self.last_save = now;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: u64 = 1_794_645_000;

    fn chrome(claude_working: bool) -> Moment<'static> {
        Moment {
            app_id: "com.google.Chrome",
            app_name: "Google Chrome",
            is_home: false,
            claude_working,
        }
    }

    fn terminal(claude_working: bool) -> Moment<'static> {
        Moment {
            app_id: "com.apple.Terminal",
            app_name: "Terminal",
            is_home: true,
            claude_working,
        }
    }

    #[test]
    fn time_goes_to_the_app_in_front_and_is_split_by_what_claude_was_doing() {
        let mut usage = Usage::default();
        usage.add_time(T0, chrome(false), 2_000);
        usage.add_time(T0, chrome(true), 3_000);
        usage.add_time(T0, terminal(true), 1_000);
        let slot = &usage.slots[&(T0 / SLOT_SECS)];
        assert_eq!(
            slot.apps["com.google.Chrome"],
            AppTime {
                ms: 5_000,
                claude_ms: 3_000
            }
        );
        assert_eq!(slot.claude_away_ms, 3_000);
        assert_eq!(slot.claude_home_ms, 1_000);
        assert_eq!(usage.names["com.apple.Terminal"], "Terminal");
    }

    #[test]
    fn the_tracker_ignores_sleep_gaps_and_idle_time() {
        let start = Instant::now();
        let mut tracker = Tracker::new(start);
        let mut usage = Usage::default();
        let at = |ms: u64| start + Duration::from_millis(ms);
        tracker.sample(
            &mut usage,
            at(500),
            T0,
            Some(chrome(false)),
            Some(Duration::ZERO),
        );
        // The laptop slept for a minute: not counted.
        tracker.sample(
            &mut usage,
            at(60_500),
            T0,
            Some(chrome(false)),
            Some(Duration::ZERO),
        );
        // Nobody has touched the machine for ten minutes: not counted.
        tracker.sample(
            &mut usage,
            at(61_000),
            T0,
            Some(chrome(false)),
            Some(Duration::from_secs(600)),
        );
        // No front app could be read: not counted.
        tracker.sample(&mut usage, at(61_500), T0, None, Some(Duration::ZERO));
        tracker.sample(&mut usage, at(62_000), T0, Some(chrome(false)), None);
        assert_eq!(
            usage.slots[&(T0 / SLOT_SECS)].apps["com.google.Chrome"].ms,
            1_000
        );
    }

    #[test]
    fn reaction_time_runs_from_the_call_to_the_return() {
        let start = Instant::now();
        let mut tracker = Tracker::new(start);
        let mut usage = Usage::default();
        let at = |s: u64| start + Duration::from_secs(s);
        tracker.log(&mut usage, "returned", at(1), T0);
        assert!(
            usage.slots.is_empty(),
            "a return with no call is not a reaction"
        );
        tracker.log(&mut usage, "return_prompt", at(10), T0);
        tracker.log(&mut usage, "return_prompt", at(15), T0);
        tracker.log(&mut usage, "returned", at(22), T0);
        let slot = &usage.slots[&(T0 / SLOT_SECS)];
        assert_eq!((slot.reactions, slot.reaction_ms), (1, 12_000));
        // A call abandoned for a new prompt, and one answered hours later, don't count.
        tracker.log(&mut usage, "return_countdown", at(30), T0);
        tracker.log(&mut usage, "run_started", at(31), T0);
        tracker.log(&mut usage, "came_back", at(40), T0);
        tracker.log(&mut usage, "return_prompt", at(50), T0);
        tracker.log(&mut usage, "came_back", at(50 + 7_200), T0);
        assert_eq!(usage.slots[&(T0 / SLOT_SECS)].reactions, 1);
    }

    #[test]
    fn old_slots_and_their_names_are_pruned() {
        let mut usage = Usage::default();
        usage.add_time(T0 - 100 * SECS_PER_DAY, terminal(false), 1_000);
        usage.add_time(T0, chrome(false), 1_000);
        usage.prune(T0, 90);
        assert_eq!(usage.slots.len(), 1);
        assert_eq!(
            usage.names.keys().collect::<Vec<_>>(),
            ["com.google.Chrome"]
        );
    }

    #[test]
    fn it_saves_privately_and_loads_back_and_survives_a_corrupt_file() {
        let dir = std::env::temp_dir().join(format!("focus-usage-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("usage.json");
        let mut usage = Usage::default();
        usage.add_time(T0, chrome(true), 4_200);
        usage.add_switch(T0);
        usage.save(&path).unwrap();
        let mut expected = usage.clone();
        expected.v = 1;
        assert_eq!(Usage::load(&path), expected);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        fs::write(&path, "{ not json").unwrap();
        assert_eq!(Usage::load(&path), Usage::default());
        assert_eq!(Usage::load(&dir.join("missing.json")), Usage::default());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn saving_is_due_only_when_something_changed_and_a_minute_passed() {
        let start = Instant::now();
        let mut tracker = Tracker::new(start);
        let mut usage = Usage::default();
        assert!(!tracker.save_due(start + Duration::from_secs(120)));
        tracker.switched(&mut usage, T0);
        assert!(!tracker.save_due(start + Duration::from_secs(30)));
        assert!(tracker.save_due(start + Duration::from_secs(61)));
        tracker.saved(start + Duration::from_secs(61));
        assert!(!tracker.save_due(start + Duration::from_secs(200)));
    }
}
