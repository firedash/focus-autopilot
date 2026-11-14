// SPDX-License-Identifier: Apache-2.0
//! Wait-time tiers (design section 9). The MVP is layer 0: no estimate, so every run is
//! `Tier::Unknown` and elapsed time alone decides. Layers 1 to 3 (V1) will set the tier.

use std::time::Duration;

use crate::config::Switching;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tier {
    Short,
    #[default]
    Unknown,
    Medium,
    Long,
}

/// How long a run must have been going before an away switch is considered.
pub fn away_threshold(tier: Tier, switching: &Switching) -> Duration {
    match tier {
        Tier::Short => switching.short_grace,
        Tier::Unknown => switching.away_after,
        Tier::Medium | Tier::Long => switching.away_after_when_long,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thresholds_follow_the_tier_table() {
        let s = Switching::default();
        assert_eq!(away_threshold(Tier::Short, &s), Duration::from_secs(45));
        assert_eq!(away_threshold(Tier::Unknown, &s), Duration::from_secs(30));
        assert_eq!(away_threshold(Tier::Medium, &s), Duration::from_secs(10));
        assert_eq!(away_threshold(Tier::Long, &s), Duration::from_secs(10));
    }
}
