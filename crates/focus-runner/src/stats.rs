// SPDX-License-Identifier: Apache-2.0
//! The statistics page's numbers: totals for today and the last seven days, per-day and
//! per-app breakdowns, and a few plain-language insights. Built from [`Usage`] and the run
//! history; a pure function of its inputs, so every number on the page is tested here.
//!
//! Days are the viewer's local days: the browser sends its offset from UTC.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use crate::history::{civil_from_days, parse_iso8601};
use crate::usage::{SLOT_SECS, Usage};

const SECS_PER_DAY: i64 = 86_400;
const DAYS_SHOWN: i64 = 7;
const APPS_SHOWN: usize = 8;
const INSIGHTS_SHOWN: usize = 6;
const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

/// One line of `runs.jsonl`, reduced to what the statistics need.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunLine {
    pub start: u64,
    pub duration_s: u64,
    pub needed_input: bool,
}

/// Every readable run in the history file, oldest first. Missing file: none.
pub fn read_runs(path: &Path) -> Vec<RunLine> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| {
            let value: Value = serde_json::from_str(line).ok()?;
            Some(RunLine {
                start: parse_iso8601(value["start"].as_str()?)?,
                duration_s: value["duration_s"].as_u64()?,
                needed_input: value["needed_input"].as_bool().unwrap_or(false),
            })
        })
        .collect()
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Totals {
    /// Prompts sent (runs started).
    pub prompts: u32,
    /// Time Claude spent working on them.
    pub claude_s: u64,
    /// While Claude worked: time the user spent in other apps, and in the Claude window.
    pub away_s: u64,
    pub home_s: u64,
    /// All time with an app in front and the user at the machine.
    pub tracked_s: u64,
    pub switches: u32,
    /// Runs that stopped to ask for approval or an answer.
    pub needed_input: u32,
    pub longest_run_s: u64,
    /// Times the user came back after being called, and the average wait.
    pub reactions: u32,
    pub reaction_avg_s: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Day {
    /// `2026-11-14`
    pub date: String,
    /// `Sat`
    pub label: String,
    pub prompts: u32,
    pub claude_s: u64,
    pub away_s: u64,
    pub tracked_s: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppRow {
    pub id: String,
    pub name: String,
    pub s: u64,
    /// The part of `s` during which Claude was working.
    pub claude_s: u64,
    pub home: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Summary {
    /// `idle`, `running`, `away` or `paused`.
    pub status: String,
    /// Whether app time is being recorded.
    pub tracking: bool,
    pub today: Totals,
    pub week: Totals,
    /// The last seven local days, oldest first, today last.
    pub days: Vec<Day>,
    pub apps_today: Vec<AppRow>,
    pub apps_week: Vec<AppRow>,
    /// Consecutive days with at least one prompt, ending today or yesterday.
    pub streak_days: u32,
    pub insights: Vec<String>,
}

pub struct Query<'a> {
    pub now_unix: u64,
    /// Minutes east of UTC.
    pub tz_minutes: i32,
    pub is_home: &'a dyn Fn(&str) -> bool,
    pub status: &'a str,
    pub tracking: bool,
}

fn local_day(unix: u64, tz_minutes: i32) -> i64 {
    let local = i64::try_from(unix).unwrap_or(i64::MAX / 2) + i64::from(tz_minutes) * 60;
    local.div_euclid(SECS_PER_DAY)
}

fn local_hour(unix: u64, tz_minutes: i32) -> u32 {
    let local = i64::try_from(unix).unwrap_or(0) + i64::from(tz_minutes) * 60;
    u32::try_from(local.rem_euclid(SECS_PER_DAY) / 3_600).unwrap_or(0)
}

fn day_label(day: i64) -> (String, String) {
    let (y, m, d) = civil_from_days(day);
    // 1970-01-01 was a Thursday.
    let weekday = usize::try_from((day + 4).rem_euclid(7)).unwrap_or(0);
    (
        format!("{y:04}-{m:02}-{d:02}"),
        WEEKDAYS[weekday].to_owned(),
    )
}

#[derive(Default)]
struct Acc {
    totals: Totals,
    reaction_ms: u64,
    apps: BTreeMap<String, (u64, u64)>,
    away_ms: u64,
    home_ms: u64,
    tracked_ms: u64,
}

impl Acc {
    fn finish(mut self, usage: &Usage, is_home: &dyn Fn(&str) -> bool) -> (Totals, Vec<AppRow>) {
        self.totals.away_s = self.away_ms / 1_000;
        self.totals.home_s = self.home_ms / 1_000;
        self.totals.tracked_s = self.tracked_ms / 1_000;
        self.totals.reaction_avg_s = (self.totals.reactions > 0)
            .then(|| self.reaction_ms / u64::from(self.totals.reactions) / 1_000);
        let mut rows: Vec<AppRow> = self
            .apps
            .into_iter()
            .filter(|(_, (ms, _))| *ms >= 1_000)
            .map(|(id, (ms, claude_ms))| AppRow {
                name: usage.names.get(&id).cloned().unwrap_or_else(|| id.clone()),
                home: is_home(&id),
                id,
                s: ms / 1_000,
                claude_s: claude_ms / 1_000,
            })
            .collect();
        rows.sort_by(|a, b| b.s.cmp(&a.s).then_with(|| a.name.cmp(&b.name)));
        rows.truncate(APPS_SHOWN);
        (self.totals, rows)
    }
}

/// Adds the runs to the totals and the per-day list. Returns prompts per local hour over the
/// seven days, and every day (within any range) that had a prompt.
fn add_runs(
    runs: &[RunLine],
    q: &Query<'_>,
    today: i64,
    today_totals: &mut Totals,
    week_totals: &mut Totals,
    days: &mut BTreeMap<i64, Day>,
) -> ([u32; 24], Vec<i64>) {
    let first = today - (DAYS_SHOWN - 1);
    let mut hours = [0_u32; 24];
    let mut active_days: Vec<i64> = Vec::new();
    for run in runs {
        let day = local_day(run.start, q.tz_minutes);
        if day <= today {
            active_days.push(day);
        }
        if day < first || day > today {
            continue;
        }
        hours[usize::try_from(local_hour(run.start, q.tz_minutes)).unwrap_or(0) % 24] += 1;
        let add = |totals: &mut Totals| {
            totals.prompts += 1;
            totals.claude_s += run.duration_s;
            totals.needed_input += u32::from(run.needed_input);
            totals.longest_run_s = totals.longest_run_s.max(run.duration_s);
        };
        add(week_totals);
        if day == today {
            add(today_totals);
        }
        if let Some(entry) = days.get_mut(&day) {
            entry.prompts += 1;
            entry.claude_s += run.duration_s;
        }
    }
    (hours, active_days)
}

/// Everything the statistics page shows.
pub fn summarize(usage: &Usage, runs: &[RunLine], q: &Query<'_>) -> Summary {
    let today = local_day(q.now_unix, q.tz_minutes);
    let first = today - (DAYS_SHOWN - 1);
    let mut today_acc = Acc::default();
    let mut week_acc = Acc::default();
    let mut days: BTreeMap<i64, Day> = (first..=today)
        .map(|day| {
            let (date, label) = day_label(day);
            (
                day,
                Day {
                    date,
                    label,
                    prompts: 0,
                    claude_s: 0,
                    away_s: 0,
                    tracked_s: 0,
                },
            )
        })
        .collect();
    let mut day_ms: BTreeMap<i64, (u64, u64)> = BTreeMap::new();

    for (slot_no, slot) in &usage.slots {
        let day = local_day(slot_no * SLOT_SECS, q.tz_minutes);
        if day < first || day > today {
            continue;
        }
        let slot_ms: u64 = slot.apps.values().map(|a| a.ms).sum();
        let entry = day_ms.entry(day).or_default();
        entry.0 += slot_ms;
        entry.1 += slot.claude_away_ms;
        let add = |acc: &mut Acc| {
            acc.totals.switches += slot.switches;
            acc.totals.reactions += slot.reactions;
            acc.reaction_ms += slot.reaction_ms;
            acc.away_ms += slot.claude_away_ms;
            acc.home_ms += slot.claude_home_ms;
            acc.tracked_ms += slot_ms;
            for (id, time) in &slot.apps {
                let app = acc.apps.entry(id.clone()).or_default();
                app.0 += time.ms;
                app.1 += time.claude_ms;
            }
        };
        add(&mut week_acc);
        if day == today {
            add(&mut today_acc);
        }
    }
    for (day, (tracked_ms, away_ms)) in day_ms {
        if let Some(entry) = days.get_mut(&day) {
            entry.tracked_s = tracked_ms / 1_000;
            entry.away_s = away_ms / 1_000;
        }
    }

    let (hours, mut active_days) = add_runs(
        runs,
        q,
        today,
        &mut today_acc.totals,
        &mut week_acc.totals,
        &mut days,
    );

    let (today_totals, apps_today) = today_acc.finish(usage, q.is_home);
    let (week_totals, apps_week) = week_acc.finish(usage, q.is_home);
    let streak_days = streak(&mut active_days, today);
    let busiest = busiest_hour(&hours);
    let insights = insights(
        &today_totals,
        &week_totals,
        &apps_week,
        streak_days,
        busiest,
    );
    Summary {
        status: q.status.to_owned(),
        tracking: q.tracking,
        today: today_totals,
        week: week_totals,
        days: days.into_values().collect(),
        apps_today,
        apps_week,
        streak_days,
        insights,
    }
}

/// Consecutive days with a prompt, ending today, or yesterday if today has none yet.
fn streak(active_days: &mut Vec<i64>, today: i64) -> u32 {
    active_days.sort_unstable();
    active_days.dedup();
    let mut expected = match active_days.last() {
        Some(&last) if last == today || last == today - 1 => last,
        _ => return 0,
    };
    let mut count = 0;
    for &day in active_days.iter().rev() {
        if day != expected {
            break;
        }
        count += 1;
        expected -= 1;
    }
    count
}

/// The hour with the most prompts, if there are enough prompts for it to mean anything.
fn busiest_hour(hours: &[u32; 24]) -> Option<u32> {
    let total: u32 = hours.iter().sum();
    let (hour, &count) = hours.iter().enumerate().max_by_key(|(_, count)| **count)?;
    (total >= 5 && count >= 2).then(|| u32::try_from(hour).unwrap_or(0))
}

/// `45 s`, `12 min`, `1 h 12 min`.
pub fn human(secs: u64) -> String {
    if secs < 60 {
        return format!("{secs} s");
    }
    // Round to minutes first, so 59 min 59 s reads as 1 h and not as 60 min.
    let minutes = (secs + 30) / 60;
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    }
}

fn hour_label(hour: u32) -> String {
    match hour {
        0 => "midnight".to_owned(),
        12 => "noon".to_owned(),
        1..12 => format!("{hour} am"),
        _ => format!("{} pm", hour - 12),
    }
}

fn plural(n: u32, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

/// The handful the page shows: a long list stops being read.
fn insights(
    today: &Totals,
    week: &Totals,
    apps_week: &[AppRow],
    streak_days: u32,
    busiest: Option<u32>,
) -> Vec<String> {
    let mut all = candidates(today, week, apps_week, streak_days, busiest);
    all.truncate(INSIGHTS_SHOWN);
    all
}

/// Every sentence the data supports, most telling first. Each needs enough data behind it to be worth
/// saying; with no data at all there is one line saying how to get some.
fn candidates(
    today: &Totals,
    week: &Totals,
    apps_week: &[AppRow],
    streak_days: u32,
    busiest: Option<u32>,
) -> Vec<String> {
    let mut out = Vec::new();
    if week.prompts == 0 && week.tracked_s == 0 {
        out.push(
            "Nothing recorded yet. Send Claude a prompt, go do something else, and this page \
             fills in."
                .to_owned(),
        );
        return out;
    }
    if today.prompts > 0 {
        out.push(format!(
            "Claude worked for {} today across {}.",
            human(today.claude_s),
            plural(today.prompts, "prompt")
        ));
    } else if week.prompts > 0 {
        out.push(format!(
            "No prompts yet today. This week Claude has worked for {} across {}.",
            human(week.claude_s),
            plural(week.prompts, "prompt")
        ));
    }
    if week.away_s >= 60 {
        out.push(format!(
            "While Claude worked this week, you got {} of other things done instead of watching \
             it.",
            human(week.away_s)
        ));
    }
    if week.home_s >= 300 && week.home_s > week.away_s {
        out.push(format!(
            "You stayed in the Claude window for {} of its working time this week. That's time \
             you could spend elsewhere.",
            human(week.home_s)
        ));
    }
    if let Some(app) = apps_week
        .iter()
        .filter(|a| !a.home && a.claude_s >= 60)
        .max_by_key(|a| a.claude_s)
    {
        out.push(format!(
            "Your go-to while waiting is {}: {} this week.",
            app.name,
            human(app.claude_s)
        ));
    }
    if let Some(avg) = week.reaction_avg_s.filter(|_| week.reactions >= 2) {
        out.push(format!(
            "When Claude calls, you're back in {} on average.",
            human(avg.max(1))
        ));
    }
    if week.longest_run_s >= 120 {
        out.push(format!(
            "Longest single run this week: {}.",
            human(week.longest_run_s)
        ));
    }
    if week.prompts >= 3 && week.needed_input > 0 {
        out.push(format!(
            "Claude stopped to ask you something in {} of {} runs this week.",
            week.needed_input, week.prompts
        ));
    }
    if today.switches >= 10 && today.tracked_s >= 600 {
        let every = today.tracked_s / u64::from(today.switches);
        out.push(format!(
            "You switched apps {} times today, about once every {}.",
            today.switches,
            human(every.max(1))
        ));
    }
    if streak_days >= 2 {
        out.push(format!(
            "You've used Claude Code {streak_days} days in a row."
        ));
    }
    if let Some(hour) = busiest {
        out.push(format!(
            "Your busiest hour for prompts this week: around {}.",
            hour_label(hour)
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::Moment;

    /// 2026-11-14 12:00:00 UTC, a Saturday.
    const NOON: u64 = 1_794_657_600;
    const DAY: u64 = 86_400;

    fn home(id: &str) -> bool {
        id == "com.apple.Terminal"
    }

    fn query(now_unix: u64, tz_minutes: i32) -> Query<'static> {
        Query {
            now_unix,
            tz_minutes,
            is_home: &home,
            status: "idle",
            tracking: true,
        }
    }

    fn moment(id: &'static str, name: &'static str, claude_working: bool) -> Moment<'static> {
        Moment {
            app_id: id,
            app_name: name,
            is_home: home(id),
            claude_working,
        }
    }

    fn run(start: u64, duration_s: u64, needed_input: bool) -> RunLine {
        RunLine {
            start,
            duration_s,
            needed_input,
        }
    }

    #[test]
    fn the_date_constant_is_what_it_says() {
        assert_eq!(
            day_label(local_day(NOON, 0)),
            ("2026-11-14".into(), "Sat".into())
        );
    }

    #[test]
    fn today_and_week_totals_add_up() {
        let mut usage = Usage::default();
        usage.add_time(
            NOON - 3_600,
            moment("com.google.Chrome", "Google Chrome", true),
            600_000,
        );
        usage.add_time(
            NOON - 3_600,
            moment("com.apple.Terminal", "Terminal", true),
            120_000,
        );
        usage.add_time(
            NOON - 3_600,
            moment("com.apple.Terminal", "Terminal", false),
            60_000,
        );
        usage.add_time(
            NOON - 2 * DAY,
            moment("com.apple.Notes", "Notes", true),
            300_000,
        );
        usage.add_time(
            NOON - 30 * DAY,
            moment("com.apple.Notes", "Notes", true),
            900_000,
        );
        usage.add_switch(NOON - 3_600);
        usage.add_reaction(NOON - 3_600, std::time::Duration::from_secs(8));
        usage.add_reaction(NOON - 2 * DAY, std::time::Duration::from_secs(20));
        let runs = [
            run(NOON - 3_600, 720, true),
            run(NOON - 1_800, 60, false),
            run(NOON - 2 * DAY, 300, false),
            run(NOON - 30 * DAY, 9_999, true),
        ];
        let s = summarize(&usage, &runs, &query(NOON, 0));
        assert_eq!(s.today.prompts, 2);
        assert_eq!(s.today.claude_s, 780);
        assert_eq!(s.today.away_s, 600);
        assert_eq!(s.today.home_s, 120);
        assert_eq!(s.today.tracked_s, 780);
        assert_eq!(s.today.needed_input, 1);
        assert_eq!(s.today.longest_run_s, 720);
        assert_eq!((s.today.reactions, s.today.reaction_avg_s), (1, Some(8)));
        assert_eq!(s.week.prompts, 3);
        assert_eq!(s.week.claude_s, 1_080);
        assert_eq!(s.week.away_s, 900);
        assert_eq!(s.week.reaction_avg_s, Some(14));
        // The run and usage from a month ago are outside both ranges.
        assert_eq!(s.week.longest_run_s, 720);
    }

    #[test]
    fn seven_days_are_listed_oldest_first_with_today_last() {
        let runs = [run(NOON, 100, false), run(NOON - 2 * DAY, 50, false)];
        let s = summarize(&Usage::default(), &runs, &query(NOON, 0));
        assert_eq!(s.days.len(), 7);
        assert_eq!(
            (s.days[6].date.as_str(), s.days[6].label.as_str()),
            ("2026-11-14", "Sat")
        );
        assert_eq!(s.days[0].label, "Sun");
        assert_eq!((s.days[6].prompts, s.days[6].claude_s), (1, 100));
        assert_eq!((s.days[4].prompts, s.days[4].claude_s), (1, 50));
        assert_eq!(s.days[5].prompts, 0);
    }

    #[test]
    fn days_follow_the_viewers_time_zone() {
        // 23:30 UTC on the 14th is already the 15th in India (+5:30) and still the 14th in UTC.
        let late = NOON + 11 * 3_600 + 1_800;
        let runs = [run(late, 100, false)];
        let utc = summarize(&Usage::default(), &runs, &query(late, 0));
        assert_eq!(utc.days[6].date, "2026-11-14");
        let india = summarize(&Usage::default(), &runs, &query(late, 330));
        assert_eq!(india.days[6].date, "2026-11-15");
        assert_eq!(india.today.prompts, 1);
        // Seen from Los Angeles (-8:00) at that moment it is the 14th, mid-afternoon.
        let la = summarize(&Usage::default(), &runs, &query(late, -480));
        assert_eq!(la.days[6].date, "2026-11-14");
        // A half-hour usage slot lands on the right local day too.
        let mut usage = Usage::default();
        usage.add_time(late, moment("com.apple.Notes", "Notes", false), 60_000);
        assert_eq!(
            summarize(&usage, &[], &query(late, 330)).today.tracked_s,
            60
        );
        assert_eq!(
            summarize(&usage, &[], &query(late + 3_600, 0))
                .today
                .tracked_s,
            0
        );
    }

    #[test]
    fn apps_are_ranked_by_time_and_marked_home_or_not() {
        let mut usage = Usage::default();
        usage.add_time(NOON, moment("com.apple.Notes", "Notes", false), 30_000);
        usage.add_time(
            NOON,
            moment("com.google.Chrome", "Google Chrome", true),
            90_000,
        );
        usage.add_time(
            NOON,
            moment("com.apple.Terminal", "Terminal", false),
            60_000,
        );
        usage.add_time(NOON, moment("com.tiny.App", "Tiny", false), 400);
        let s = summarize(&usage, &[], &query(NOON, 0));
        let names: Vec<&str> = s.apps_today.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["Google Chrome", "Terminal", "Notes"]);
        assert_eq!((s.apps_today[0].s, s.apps_today[0].claude_s), (90, 90));
        assert!(s.apps_today[1].home && !s.apps_today[0].home);
    }

    #[test]
    fn the_streak_counts_back_from_today_or_yesterday() {
        let days = |offsets: &[u64]| -> Vec<RunLine> {
            offsets
                .iter()
                .map(|d| run(NOON - d * DAY, 10, false))
                .collect()
        };
        let streak_of =
            |runs: &[RunLine]| summarize(&Usage::default(), runs, &query(NOON, 0)).streak_days;
        assert_eq!(streak_of(&days(&[0, 1, 2, 4])), 3);
        assert_eq!(streak_of(&days(&[1, 2])), 2);
        assert_eq!(streak_of(&days(&[2, 3])), 0);
        assert_eq!(streak_of(&days(&[0, 0, 0])), 1);
        assert_eq!(streak_of(&[]), 0);
    }

    #[test]
    fn durations_read_naturally() {
        assert_eq!(human(0), "0 s");
        assert_eq!(human(45), "45 s");
        assert_eq!(human(60), "1 min");
        assert_eq!(human(725), "12 min");
        assert_eq!(human(3_600), "1 h");
        assert_eq!(human(4_320), "1 h 12 min");
        assert_eq!(human(3_599), "1 h");
        assert_eq!(hour_label(0), "midnight");
        assert_eq!(hour_label(9), "9 am");
        assert_eq!(hour_label(12), "noon");
        assert_eq!(hour_label(15), "3 pm");
    }

    #[test]
    fn with_no_data_there_is_one_line_saying_how_to_get_some() {
        let s = summarize(&Usage::default(), &[], &query(NOON, 0));
        assert_eq!(s.insights.len(), 1);
        assert!(s.insights[0].starts_with("Nothing recorded yet."));
    }

    #[test]
    fn insights_say_what_the_numbers_say() {
        let mut usage = Usage::default();
        usage.add_time(
            NOON - 3_600,
            moment("com.google.Chrome", "Google Chrome", true),
            1_860_000,
        );
        usage.add_time(
            NOON - 3_600,
            moment("com.apple.Terminal", "Terminal", true),
            60_000,
        );
        for _ in 0..12 {
            usage.add_switch(NOON - 3_600);
        }
        usage.add_reaction(NOON - 3_600, std::time::Duration::from_secs(10));
        usage.add_reaction(NOON - 3_600, std::time::Duration::from_secs(14));
        let mut runs = vec![run(NOON - 3_600, 4_320, true)];
        runs.extend((0..8).map(|i| run(NOON - 7_200 - i * 60, 30, false)));
        runs.push(run(NOON - DAY, 60, false));
        let s = summarize(&usage, &runs, &query(NOON, 0));
        let has = |text: &str| {
            assert!(
                s.insights.iter().any(|i| i == text),
                "missing {text:?} in {:#?}",
                s.insights
            );
        };
        has("Claude worked for 1 h 16 min today across 9 prompts.");
        has(
            "While Claude worked this week, you got 31 min of other things done instead of watching it.",
        );
        has("Your go-to while waiting is Google Chrome: 31 min this week.");
        has("When Claude calls, you're back in 12 s on average.");
        has("Longest single run this week: 1 h 12 min.");
        has("Claude stopped to ask you something in 1 of 10 runs this week.");
        // Six are shown; the rest are still true, and are next in line.
        assert_eq!(s.insights.len(), INSIGHTS_SHOWN);
        let all = candidates(&s.today, &s.week, &s.apps_week, s.streak_days, Some(9));
        assert_eq!(
            all[INSIGHTS_SHOWN..],
            [
                "You switched apps 12 times today, about once every 3 min.",
                "You've used Claude Code 2 days in a row.",
                "Your busiest hour for prompts this week: around 9 am.",
            ]
        );
    }

    #[test]
    fn the_busiest_hour_needs_enough_prompts_to_mean_anything() {
        let mut hours = [0_u32; 24];
        hours[9] = 7;
        hours[10] = 1;
        assert_eq!(busiest_hour(&hours), Some(9));
        hours[9] = 1;
        assert_eq!(busiest_hour(&hours), None, "two prompts are not a pattern");
        assert_eq!(busiest_hour(&[0; 24]), None);
    }

    #[test]
    fn thin_data_does_not_produce_grand_claims() {
        let mut usage = Usage::default();
        usage.add_time(
            NOON,
            moment("com.google.Chrome", "Google Chrome", true),
            20_000,
        );
        usage.add_reaction(NOON, std::time::Duration::from_secs(3));
        let s = summarize(&usage, &[run(NOON, 20, true)], &query(NOON, 0));
        assert_eq!(
            s.insights,
            ["Claude worked for 20 s today across 1 prompt."]
        );
    }

    #[test]
    fn runs_are_read_from_the_history_file_and_bad_lines_skipped() {
        let dir = std::env::temp_dir().join(format!("focus-stats-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("runs.jsonl");
        std::fs::write(
            &path,
            "{\"start\":\"2026-11-14T12:00:00Z\",\"duration_s\":184,\"needed_input\":true,\"outcome\":\"done\"}\nnot json\n{\"start\":\"bad\",\"duration_s\":1}\n",
        )
        .unwrap();
        assert_eq!(read_runs(&path), [run(NOON, 184, true)]);
        assert!(read_runs(&dir.join("none.jsonl")).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }
}
