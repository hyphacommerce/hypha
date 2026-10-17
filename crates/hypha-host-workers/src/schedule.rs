//! Which jobs a cron tick runs. The Worker has one trigger, every minute (`TICK`,
//! `wrangler.toml`), and each job runs on the ticks that start its interval
//! (`jobs.<job>.interval`, as natively), so no cron string is matched: prune, hourly by
//! default, runs at :00. A job whose interval is shorter than a tick runs every tick.
//! Portable, so its tests run natively.

use std::time::Duration;

/// How often the cron trigger fires.
pub const TICK: Duration = Duration::from_secs(60);

/// Whether the tick scheduled at `at_millis` (since the epoch) starts a new `interval`.
pub fn due(at_millis: u64, interval: Duration) -> bool {
    let tick = TICK.as_millis() as u64;
    let interval = u64::try_from(interval.as_millis())
        .unwrap_or(u64::MAX)
        .max(1);
    interval <= tick || at_millis / interval != at_millis.saturating_sub(tick) / interval
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINUTE: u64 = 60_000;
    const HOUR: Duration = Duration::from_secs(3600);

    #[test]
    fn an_hourly_job_runs_on_the_hours_first_tick() {
        let noon = 12 * 60 * MINUTE;
        assert!(due(noon, HOUR));
        assert!(!due(noon + MINUTE, HOUR));
        assert!(!due(noon - MINUTE, HOUR));
    }

    #[test]
    fn a_late_tick_still_counts_for_its_hour() {
        let noon = 12 * 60 * MINUTE;
        assert!(due(noon + 30_000, HOUR));
        assert!(!due(noon + MINUTE + 30_000, HOUR));
    }

    #[test]
    fn a_job_shorter_than_a_tick_runs_every_tick() {
        assert!(due(7 * MINUTE + 1, Duration::from_secs(2)));
        assert!(due(7 * MINUTE, TICK));
    }
}
