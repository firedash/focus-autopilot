// SPDX-License-Identifier: Apache-2.0
//! Wait-time estimation (design section 9). Claude Code gives no ETA, so the estimate is
//! built in layers, each optional: elapsed time alone (layer 0), the median of similar past
//! runs (layer 1), floors raised by live events (layer 2), and the user's own call (layer 3).
//! The output is a tier and a range, never one confident number.

use std::collections::VecDeque;
use std::time::Duration;

use crate::config::Switching;

/// Tier boundaries from design section 2.
const SHORT_UNDER: Duration = Duration::from_secs(30);
const LONG_FROM: Duration = Duration::from_secs(300);
/// Layer 1: how many recent similar runs are used, and how few are too few.
const GROUP_WINDOW: usize = 20;
const MIN_SAMPLES: usize = 5;
/// Samples held in memory; the shell trims the file on disk by age.
const MAX_SAMPLES: usize = 5_000;
/// Layer 2 floors. Starting guesses, to be tuned from real logs.
pub(crate) const SUBAGENT_FLOOR: Duration = Duration::from_secs(300);
pub(crate) const SLOW_COMMAND_FLOOR: Duration = Duration::from_secs(120);
pub(crate) const MANY_TOOLS_FLOOR: Duration = Duration::from_secs(180);
pub(crate) const MANY_TOOLS: u32 = 15;
/// A Bash or PowerShell command starting with one of these is a test, build or install.
const SLOW_TOOLS: [&str; 10] = [
    "pytest",
    "npm",
    "pnpm",
    "yarn",
    "cargo",
    "gradle",
    "mvn",
    "dotnet",
    "xcodebuild",
    "make",
];
const SLOW_SUBCOMMANDS: [(&str, &str); 2] = [("go", "test"), ("docker", "build")];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tier {
    Short,
    #[default]
    Unknown,
    Medium,
    Long,
}

/// Prompt length bucket used to group similar runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PromptLen {
    /// Under 200 characters.
    #[default]
    Short,
    /// 200 to 1,000 characters.
    Medium,
    /// Over 1,000 characters.
    Long,
}

impl PromptLen {
    pub fn of_chars(chars: usize) -> Self {
        match chars {
            0..200 => PromptLen::Short,
            200..=1_000 => PromptLen::Medium,
            _ => PromptLen::Long,
        }
    }
}

/// One finished run, as the estimator sees it. No prompt text, paths or commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSample {
    /// Salted hash of the project folder, computed by the shell.
    pub project: String,
    pub permission_mode: Option<String>,
    pub prompt_len: PromptLen,
    pub duration: Duration,
}

/// The median of similar past runs with its 25th to 75th percentile range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Estimate {
    pub median: Duration,
    pub low: Duration,
    pub high: Duration,
}

/// How long a run must have been going before an away switch is considered.
pub fn away_threshold(tier: Tier, switching: &Switching) -> Duration {
    match tier {
        Tier::Short => switching.short_grace,
        Tier::Unknown => switching.away_after,
        Tier::Medium | Tier::Long => switching.away_after_when_long,
    }
}

pub fn tier_of(expected: Duration) -> Tier {
    if expected < SHORT_UNDER {
        Tier::Short
    } else if expected < LONG_FROM {
        Tier::Medium
    } else {
        Tier::Long
    }
}

pub(crate) fn is_slow_command(command: &str) -> bool {
    let mut words = command.split_whitespace();
    let Some(first) = words.next() else {
        return false;
    };
    // `./gradlew` and `C:\tools\npm.cmd` count as their tool.
    let tool = first.rsplit(['/', '\\']).next().unwrap_or(first);
    let tool = tool.split('.').next().unwrap_or(tool);
    if SLOW_TOOLS.contains(&tool) || tool == "gradlew" {
        return true;
    }
    let second = words.next();
    SLOW_SUBCOMMANDS
        .iter()
        .any(|(t, sub)| tool == *t && second == Some(*sub))
}

/// Past runs, newest last.
#[derive(Debug, Default)]
pub(crate) struct History {
    samples: VecDeque<RunSample>,
}

impl History {
    pub fn replace(&mut self, samples: Vec<RunSample>) {
        self.samples = samples.into();
        while self.samples.len() > MAX_SAMPLES {
            self.samples.pop_front();
        }
    }

    pub fn push(&mut self, sample: RunSample) {
        if self.samples.len() == MAX_SAMPLES {
            self.samples.pop_front();
        }
        self.samples.push_back(sample);
    }

    /// Layer 1: the last 20 runs in the same project, permission mode and prompt-length
    /// bucket; with fewer than 5 of those, every run with the same prompt length.
    pub fn estimate(
        &self,
        project: &str,
        permission_mode: Option<&str>,
        prompt_len: PromptLen,
    ) -> Option<Estimate> {
        let same_len = |s: &&RunSample| s.prompt_len == prompt_len;
        let same_group = |s: &&RunSample| {
            s.project == project && s.permission_mode.as_deref() == permission_mode
        };
        let group: Vec<Duration> = self
            .samples
            .iter()
            .rev()
            .filter(same_len)
            .filter(same_group)
            .take(GROUP_WINDOW)
            .map(|s| s.duration)
            .collect();
        if group.len() >= MIN_SAMPLES {
            return stats(group);
        }
        let all: Vec<Duration> = self
            .samples
            .iter()
            .filter(same_len)
            .map(|s| s.duration)
            .collect();
        if all.len() >= MIN_SAMPLES {
            stats(all)
        } else {
            None
        }
    }
}

fn stats(mut durations: Vec<Duration>) -> Option<Estimate> {
    durations.sort_unstable();
    let at = |numerator: usize, denominator: usize| {
        let index = (durations.len().saturating_sub(1) * numerator + denominator / 2) / denominator;
        durations.get(index).copied()
    };
    Some(Estimate {
        median: at(1, 2)?,
        low: at(1, 4)?,
        high: at(3, 4)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(project: &str, len: PromptLen, secs: u64) -> RunSample {
        RunSample {
            project: project.into(),
            permission_mode: Some("default".into()),
            prompt_len: len,
            duration: Duration::from_secs(secs),
        }
    }

    #[test]
    fn thresholds_follow_the_tier_table() {
        let s = Switching::default();
        assert_eq!(away_threshold(Tier::Short, &s), Duration::from_secs(45));
        assert_eq!(away_threshold(Tier::Unknown, &s), Duration::from_secs(30));
        assert_eq!(away_threshold(Tier::Medium, &s), Duration::from_secs(10));
        assert_eq!(away_threshold(Tier::Long, &s), Duration::from_secs(10));
    }

    #[test]
    fn tiers_and_prompt_buckets_have_the_documented_edges() {
        assert_eq!(tier_of(Duration::from_secs(29)), Tier::Short);
        assert_eq!(tier_of(Duration::from_secs(30)), Tier::Medium);
        assert_eq!(tier_of(Duration::from_secs(299)), Tier::Medium);
        assert_eq!(tier_of(Duration::from_secs(300)), Tier::Long);
        assert_eq!(PromptLen::of_chars(199), PromptLen::Short);
        assert_eq!(PromptLen::of_chars(200), PromptLen::Medium);
        assert_eq!(PromptLen::of_chars(1_000), PromptLen::Medium);
        assert_eq!(PromptLen::of_chars(1_001), PromptLen::Long);
    }

    #[test]
    fn slow_commands_are_recognised_by_their_first_words() {
        for slow in [
            "npm test",
            "cargo build --release",
            "go test ./...",
            "docker build .",
            "./gradlew assemble",
            "  pytest -x",
            r"C:\tools\npm.cmd install",
            "make",
        ] {
            assert!(is_slow_command(slow), "{slow}");
        }
        for quick in [
            "ls -la",
            "go version",
            "docker ps",
            "git status",
            "echo npm",
            "",
        ] {
            assert!(!is_slow_command(quick), "{quick}");
        }
    }

    #[test]
    fn history_needs_five_similar_runs_and_uses_the_latest_twenty() {
        let mut history = History::default();
        for secs in [10, 20, 30, 40] {
            history.push(sample("a", PromptLen::Short, secs));
        }
        assert_eq!(
            history.estimate("a", Some("default"), PromptLen::Short),
            None
        );
        history.push(sample("a", PromptLen::Short, 50));
        let estimate = history
            .estimate("a", Some("default"), PromptLen::Short)
            .unwrap();
        assert_eq!(estimate.median, Duration::from_secs(30));
        assert_eq!(
            (estimate.low, estimate.high),
            (Duration::from_secs(20), Duration::from_secs(40))
        );
        // Another project falls back to every run with the same prompt length.
        assert_eq!(
            history.estimate("b", Some("default"), PromptLen::Short),
            Some(estimate)
        );
        assert_eq!(
            history.estimate("b", Some("default"), PromptLen::Long),
            None
        );
        // Only the newest twenty runs of the group count.
        for _ in 0..20 {
            history.push(sample("a", PromptLen::Short, 600));
        }
        let estimate = history
            .estimate("a", Some("default"), PromptLen::Short)
            .unwrap();
        assert_eq!(estimate.median, Duration::from_secs(600));
    }

    #[test]
    fn history_is_bounded_and_replaceable() {
        let mut history = History::default();
        history.replace(
            (0..MAX_SAMPLES + 10)
                .map(|_| sample("a", PromptLen::Short, 1))
                .collect(),
        );
        assert_eq!(history.samples.len(), MAX_SAMPLES);
        history.push(sample("a", PromptLen::Short, 2));
        assert_eq!(history.samples.len(), MAX_SAMPLES);
        history.replace(Vec::new());
        assert_eq!(
            history.estimate("a", Some("default"), PromptLen::Short),
            None
        );
    }
}
