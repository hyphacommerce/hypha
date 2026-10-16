//! When a failed delivery is tried again (design note 0016, Q6). By default: after 1
//! minute, 5 minutes, 30 minutes, 2 hours and 6 hours, then every 12 hours, for about 3
//! days in all; then it fails for good, and can be retried from the admin. Configurable
//! as `[webhooks]` (design note 0017).

use core::time::Duration;

use hypha_core::Timestamp;

const MINUTE: Duration = Duration::from_secs(60);
const HOUR: Duration = Duration::from_secs(3600);

/// The waits between attempts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetrySchedule {
    /// The waits after the first failures, in order.
    pub backoff: Vec<Duration>,
    /// The wait after each later failure.
    pub then_every: Duration,
    /// No attempt is planned more than this long after the first.
    pub give_up_after: Duration,
}

impl Default for RetrySchedule {
    fn default() -> Self {
        Self {
            backoff: vec![MINUTE, 5 * MINUTE, 30 * MINUTE, 2 * HOUR, 6 * HOUR],
            then_every: 12 * HOUR,
            give_up_after: 72 * HOUR,
        }
    }
}

impl RetrySchedule {
    /// How long to wait after the `failures`-th failure (1 for the first).
    fn wait_after(&self, failures: u32) -> Duration {
        match failures {
            0 => Duration::ZERO,
            n => self
                .backoff
                .get(n as usize - 1)
                .copied()
                .unwrap_or(self.then_every),
        }
    }

    /// When to try again after the `failures`-th failure, at `now`; `None` once the
    /// attempts would run past `give_up_after`, when the delivery fails for good.
    pub fn next_attempt(&self, failures: u32, now: Timestamp) -> Option<Timestamp> {
        let elapsed: Duration = (1..=failures).map(|n| self.wait_after(n)).sum();
        (elapsed <= self.give_up_after).then(|| now.plus(self.wait_after(failures)))
    }

    /// Attempts a delivery gets in all, the first included.
    pub fn max_attempts(&self) -> u32 {
        // The first failure whose waits add up past `give_up_after`. Every wait is above
        // zero (the configuration checks it), so this ends; the cap is for a zero anyway.
        (1..=10_000)
            .find(|&n| self.next_attempt(n, Timestamp::UNIX_EPOCH).is_none())
            .unwrap_or(10_000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: Timestamp = Timestamp::from_unix_micros(1_800_000_000_000_000);

    #[test]
    fn the_first_retries_back_off() {
        let schedule = RetrySchedule::default();
        let waits: Vec<i64> = (1..=6)
            .map(|n| {
                (schedule.next_attempt(n, NOW).unwrap().unix_micros() - NOW.unix_micros())
                    / 1_000_000
            })
            .collect();
        assert_eq!(waits, [60, 300, 1800, 7200, 21600, 43200]);
    }

    #[test]
    fn it_gives_up_after_about_three_days() {
        let schedule = RetrySchedule::default();
        assert_eq!(schedule.max_attempts(), 11);
        assert!(schedule.next_attempt(10, NOW).is_some());
        assert_eq!(schedule.next_attempt(11, NOW), None);
    }

    #[test]
    fn a_shorter_schedule_gives_up_sooner() {
        let schedule = RetrySchedule {
            backoff: vec![MINUTE],
            then_every: MINUTE,
            give_up_after: 2 * MINUTE,
        };
        assert_eq!(schedule.next_attempt(2, NOW), Some(NOW.plus(MINUTE)));
        assert_eq!(schedule.next_attempt(3, NOW), None);
        assert_eq!(schedule.max_attempts(), 3);
    }
}
