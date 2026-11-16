// SPDX-License-Identifier: Apache-2.0
//! `runs.jsonl` (design section 11): one metadata line per finished run, so the estimator
//! learns across launches. No prompt text, commands or paths are ever written; the project is
//! a salted hash. Lines older than the retention period are dropped when the file is loaded.

use std::fs;
use std::io::Write as _;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use focus_core::{PromptLen, RunOutcome, RunRecord, RunSample};
use serde_json::{Value, json};

const SECS_PER_DAY: u64 = 86_400;

/// One JSON line for `record`, which started `ago` before `now_unix` seconds since 1970.
pub fn line(record: &RunRecord, now_unix: u64, ago: Duration) -> String {
    let start = now_unix.saturating_sub(ago.as_secs());
    let outcome = match record.outcome {
        RunOutcome::Done => "done",
        RunOutcome::Error => "error",
        RunOutcome::Stale => "stale",
    };
    let lower = |debug: String| debug.to_lowercase();
    json!({
        "v": 1,
        "run_id": record.run_id,
        "project": record.project,
        "start": iso8601(start),
        "duration_s": record.duration.as_secs(),
        "outcome": outcome,
        "permission_mode": record.permission_mode,
        "effort": record.effort,
        "prompt_len": lower(format!("{:?}", record.prompt_len)),
        "tools": record.tools,
        "subagents": record.subagents,
        "slow_commands": record.slow_commands,
        "needed_input": record.needed_input,
        "predicted_tier": lower(format!("{:?}", record.predicted_tier)),
        "away": lower(format!("{:?}", record.away)),
        "return": lower(format!("{:?}", record.returned)),
        "snoozes": record.snoozes,
    })
    .to_string()
}

/// Appends a record. Errors are returned for the log; a failed write never stops the app.
///
/// # Errors
/// When the folder or file can't be created or written.
pub fn append(path: &Path, record: &RunRecord, started_at: Instant) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let ago = Instant::now().saturating_duration_since(started_at);
    let text = line(record, unix_now(), ago);
    let mut options = fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    writeln!(file, "{text}").map_err(|e| format!("{}: {e}", path.display()))
}

/// Past runs for `Input::History`, oldest first. Only runs that ended with a Stop count, since
/// stale and failed runs say nothing about how long such a run takes. Unreadable lines are
/// skipped. Lines older than `retention_days` are removed from the file.
pub fn load(path: &Path, retention_days: u32) -> Vec<RunSample> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let oldest = unix_now().saturating_sub(u64::from(retention_days) * SECS_PER_DAY);
    let mut samples = Vec::new();
    let mut kept = String::new();
    let mut dropped = false;
    for raw in text.lines().filter(|l| !l.trim().is_empty()) {
        let Ok(value) = serde_json::from_str::<Value>(raw) else {
            dropped = true;
            continue;
        };
        let at = value["start"].as_str().and_then(parse_iso8601);
        if at.is_some_and(|at| at < oldest) {
            dropped = true;
            continue;
        }
        kept.push_str(raw);
        kept.push('\n');
        if let Some(sample) = sample(&value) {
            samples.push(sample);
        }
    }
    if dropped {
        // Best effort: an old line left behind is harmless.
        let _ = fs::write(path, kept);
    }
    samples
}

fn sample(value: &Value) -> Option<RunSample> {
    if value["outcome"].as_str() != Some("done") {
        return None;
    }
    let prompt_len = match value["prompt_len"].as_str()? {
        "short" => PromptLen::Short,
        "medium" => PromptLen::Medium,
        "long" => PromptLen::Long,
        _ => return None,
    };
    Some(RunSample {
        project: value["project"].as_str()?.to_owned(),
        permission_mode: value["permission_mode"].as_str().map(str::to_owned),
        prompt_len,
        duration: Duration::from_secs(value["duration_s"].as_u64()?),
    })
}

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// `2026-11-14T08:31:02Z` from seconds since 1970 (UTC).
pub fn iso8601(unix: u64) -> String {
    let days = i64::try_from(unix / SECS_PER_DAY).unwrap_or(0);
    let rest = unix % SECS_PER_DAY;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

pub fn parse_iso8601(text: &str) -> Option<u64> {
    let b = text.as_bytes();
    if b.len() != 20 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[19] != b'Z' {
        return None;
    }
    let num = |a: usize, z: usize| text.get(a..z)?.parse::<i64>().ok();
    let days = days_from_civil(num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let secs = num(11, 13)? * 3600 + num(14, 16)? * 60 + num(17, 19)?;
    u64::try_from(days * 86_400 + secs).ok()
}

// Howard Hinnant's civil-date algorithms, valid for the proleptic Gregorian calendar.
pub(crate) fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = y - i64::from(m <= 2);
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;
    use focus_core::{AwayHow, ReturnHow, Tier};

    fn record(outcome: RunOutcome, secs: u64) -> RunRecord {
        RunRecord {
            run_id: Some("p1".into()),
            project: "abc123".into(),
            started: Instant::now(),
            duration: Duration::from_secs(secs),
            outcome,
            permission_mode: Some("default".into()),
            effort: None,
            prompt_len: PromptLen::Medium,
            tools: 3,
            subagents: 0,
            slow_commands: 1,
            needed_input: true,
            predicted_tier: Tier::Long,
            away: AwayHow::Auto,
            returned: ReturnHow::Prompt,
            snoozes: 0,
        }
    }

    fn dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("focus-history-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn dates_round_trip_through_known_values() {
        assert_eq!(iso8601(1_794_645_062), "2026-11-14T08:31:02Z");
        assert_eq!(parse_iso8601("2026-11-14T08:31:02Z"), Some(1_794_645_062));
        assert_eq!(iso8601(951_868_799), "2000-02-29T23:59:59Z");
        assert_eq!(parse_iso8601("2000-02-29T23:59:59Z"), Some(951_868_799));
        assert_eq!(parse_iso8601("yesterday"), None);
    }

    #[test]
    fn a_line_has_the_designs_fields_and_no_text() {
        let text = line(
            &record(RunOutcome::Done, 184),
            1_794_645_062,
            Duration::from_secs(184),
        );
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["v"], 1);
        assert_eq!(v["start"], "2026-11-14T08:27:58Z");
        assert_eq!(v["duration_s"], 184);
        assert_eq!(v["outcome"], "done");
        assert_eq!(v["prompt_len"], "medium");
        assert_eq!(v["predicted_tier"], "long");
        assert_eq!(v["away"], "auto");
        assert_eq!(v["return"], "prompt");
        for field in ["prompt", "command", "cwd", "message"] {
            assert!(v.get(field).is_none(), "{field} must never be written");
        }
    }

    #[test]
    fn appended_runs_load_back_and_only_finished_ones_count() {
        let d = dir("roundtrip");
        let path = d.join("runs.jsonl");
        append(&path, &record(RunOutcome::Done, 120), Instant::now()).unwrap();
        append(&path, &record(RunOutcome::Stale, 5), Instant::now()).unwrap();
        append(&path, &record(RunOutcome::Done, 300), Instant::now()).unwrap();
        let samples = load(&path, 90);
        assert_eq!(samples.len(), 2);
        assert_eq!(samples[0].duration, Duration::from_secs(120));
        assert_eq!(samples[1].project, "abc123");
        assert_eq!(samples[1].prompt_len, PromptLen::Medium);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn old_and_corrupt_lines_are_dropped_from_the_file() {
        let d = dir("retention");
        fs::create_dir_all(&d).unwrap();
        let path = d.join("runs.jsonl");
        let fresh = line(&record(RunOutcome::Done, 60), unix_now(), Duration::ZERO);
        let old = line(
            &record(RunOutcome::Done, 60),
            unix_now() - 200 * SECS_PER_DAY,
            Duration::ZERO,
        );
        fs::write(&path, format!("{old}\nnot json\n{fresh}\n")).unwrap();
        assert_eq!(load(&path, 90).len(), 1);
        let rewritten = fs::read_to_string(&path).unwrap();
        assert_eq!(rewritten.lines().count(), 1);
        assert!(!rewritten.contains("not json"));
        assert!(load(&d.join("missing.jsonl"), 90).is_empty());
        let _ = fs::remove_dir_all(d);
    }
}
