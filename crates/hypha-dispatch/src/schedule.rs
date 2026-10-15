//! When a failed delivery is tried again (design note 0016, Q6): after 1 minute, 5
//! minutes, 30 minutes, 2 hours and 6 hours, then every 12 hours, for about 3 days in
//! all; then it fails for good, and can be retried from the admin.

use hypha_core::Timestamp;

/// The waits after the first failures, in seconds.
const BACKOFF_SECS: [i64; 5] = [60, 5 * 60, 30 * 60, 2 * 3600, 6 * 3600];
/// The wait after each later failure.
const THEN_SECS: i64 = 12 * 3600;
/// No attempt is planned more than this long after the first.
const GIVE_UP_AFTER_SECS: i64 = 3 * 24 * 3600;

/// How long to wait after the `failures`-th failure (1 for the first).
fn wait_after(failures: u32) -> i64 {
    match failures {
        0 => 0,
        n => BACKOFF_SECS
            .get(n as usize - 1)
            .copied()
            .unwrap_or(THEN_SECS),
    }
}

/// When to try again after the `failures`-th failure, at `now`; `None` once the
/// attempts would run past about 3 days, when the delivery fails for good.
pub fn next_attempt(failures: u32, now: Timestamp) -> Option<Timestamp> {
    let elapsed: i64 = (1..=failures).map(wait_after).sum();
    (elapsed <= GIVE_UP_AFTER_SECS).then(|| now.plus_secs(wait_after(failures)))
}

/// Attempts a delivery gets in all, the first included.
pub const MAX_ATTEMPTS: u32 = 11;

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: Timestamp = Timestamp::from_unix_micros(1_800_000_000_000_000);

    #[test]
    fn the_first_retries_back_off() {
        let waits: Vec<i64> = (1..=6)
            .map(|n| (next_attempt(n, NOW).unwrap().unix_micros() - NOW.unix_micros()) / 1_000_000)
            .collect();
        assert_eq!(waits, [60, 300, 1800, 7200, 21600, 43200]);
    }

    #[test]
    fn it_gives_up_after_about_three_days() {
        assert!(next_attempt(MAX_ATTEMPTS - 1, NOW).is_some());
        assert_eq!(next_attempt(MAX_ATTEMPTS, NOW), None);
    }
}
