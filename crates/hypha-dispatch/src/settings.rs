//! The dispatcher's settings: `[jobs.dispatch]`, `[jobs.prune]` and `[webhooks]` in
//! `hypha.config.toml`, defaults here.

use core::time::Duration;

use crate::schedule::RetrySchedule;

const DAY: Duration = Duration::from_secs(24 * 3600);

/// How much the jobs take on at once, how long a lease lasts, when failures are retried
/// and how long delivered history is kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchSettings {
    /// Events fanned out, deliveries run, or rows pruned per call.
    pub batch: u32,
    /// How long a delivery is leased for: past this, another runner may take it. At
    /// least `LONGEST_RUN`, or no delivery ever starts (the configuration checks it).
    pub lease: Duration,
    pub retry: RetrySchedule,
    /// How long events (and their deliveries) are kept.
    pub keep_events: Duration,
    /// How long an idempotency key is honoured.
    pub keep_idempotency_keys: Duration,
    /// How long a session is kept after it ended (expired or revoked).
    pub keep_ended_sessions: Duration,
}

impl Default for DispatchSettings {
    fn default() -> Self {
        Self {
            batch: 100,
            lease: Duration::from_secs(120),
            retry: RetrySchedule::default(),
            keep_events: 30 * DAY,
            keep_idempotency_keys: DAY,
            keep_ended_sessions: 30 * DAY,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_is_kept_thirty_days_and_keys_one() {
        let days = |n: u64| Duration::from_secs(n * 86_400);
        let defaults = DispatchSettings::default();
        assert_eq!(defaults.keep_events, days(30));
        assert_eq!(defaults.keep_idempotency_keys, days(1));
        assert_eq!(defaults.keep_ended_sessions, days(30));
    }
}
