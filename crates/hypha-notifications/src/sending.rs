//! The send job's settings (design note 0028, §4; 0017): `[jobs.send_notifications]` in
//! `hypha.config.toml`, defaults here.

use core::time::Duration;

use hypha_core::Timestamp;

/// How long a sender waits for the provider: `hypha-mail-resend`'s request timeout, the
/// Workers `fetch`'s, the SMTP sender's for each command. Short enough that a send fits
/// in what's left of a Workers `waitUntil` after a response.
pub const SEND_TIMEOUT: Duration = Duration::from_secs(10);

/// The longest one send may take: `SEND_TIMEOUT` with room to connect and build the
/// request. A runner doesn't start a send with less of its lease (or its host's time)
/// left, so a lease never runs out while its message is with the provider.
pub const LONGEST_SEND: Duration = Duration::from_secs(SEND_TIMEOUT.as_secs() + 5);

const MINUTE: Duration = Duration::from_secs(60);
const HOUR: Duration = Duration::from_secs(3600);

/// How much the send job takes on at once, how long it holds it, and when a message the
/// provider couldn't take is tried again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendSettings {
    /// Messages leased per step.
    pub batch: u32,
    /// How long the job holds the messages it leased; past it, another run takes them.
    /// At least `LONGEST_SEND` plus room to record (the configuration checks it).
    pub lease: Duration,
    /// The waits after each transient failure, in order; after the last, the message
    /// fails for good (staff can resend it). Empty: no retries.
    pub retry: Vec<Duration>,
}

impl Default for SendSettings {
    fn default() -> Self {
        Self {
            batch: 20,
            lease: 2 * MINUTE,
            retry: vec![MINUTE, 5 * MINUTE, 30 * MINUTE, 2 * HOUR, 6 * HOUR],
        }
    }
}

impl SendSettings {
    /// When to try again after the `attempts`-th attempt (1 for the first) failed at
    /// `now`; `None` once the schedule is used up: it fails for good.
    pub fn next_attempt(&self, attempts: u32, now: Timestamp) -> Option<Timestamp> {
        let index = usize::try_from(attempts).ok()?.checked_sub(1)?;
        self.retry.get(index).map(|wait| now.plus(*wait))
    }

    /// Attempts a message gets in all, the first included.
    pub fn max_attempts(&self) -> usize {
        self.retry.len() + 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: Timestamp = Timestamp::from_unix_micros(1_800_000_000_000_000);

    #[test]
    fn failures_back_off_then_give_up() {
        let settings = SendSettings::default();
        let waits: Vec<Option<i64>> = (1..=6)
            .map(|n| {
                settings
                    .next_attempt(n, NOW)
                    .map(|at| (at.unix_micros() - NOW.unix_micros()) / 1_000_000)
            })
            .collect();
        assert_eq!(
            waits,
            [
                Some(60),
                Some(300),
                Some(1800),
                Some(7200),
                Some(21600),
                None
            ]
        );
        assert_eq!(settings.max_attempts(), 6);
    }

    #[test]
    fn without_retries_the_first_failure_is_final() {
        let settings = SendSettings {
            retry: Vec::new(),
            ..SendSettings::default()
        };
        assert_eq!(settings.next_attempt(1, NOW), None);
        assert_eq!(settings.next_attempt(0, NOW), None);
    }
}
