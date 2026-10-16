//! The dispatcher's settings (design note 0017): `[jobs.dispatch]`, `[jobs.prune]` and
//! `[webhooks]` in `hypha.config.toml`, defaults here.

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
    /// How long events (and their deliveries) are kept (0016, Q7).
    pub keep_events: Duration,
    /// How long an idempotency key is honoured (design note 0007, section 5).
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
