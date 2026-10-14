use std::sync::atomic::{AtomicI64, Ordering};

use hypha_core::{Clock, Timestamp};

/// A clock that stands still until the test moves it, so every timestamp a command
/// writes is known exactly. `default()` is 2026-01-01T00:00:00Z.
pub struct FixedClock {
    now: AtomicI64,
}

impl FixedClock {
    /// 2026-01-01T00:00:00Z, in microseconds since the Unix epoch.
    pub const DEFAULT: Timestamp = Timestamp::from_unix_micros(1_767_225_600_000_000);

    pub fn at(now: Timestamp) -> Self {
        Self {
            now: AtomicI64::new(now.unix_micros()),
        }
    }

    /// Moves the clock to `now`, forwards or back.
    pub fn set(&self, now: Timestamp) {
        self.now.store(now.unix_micros(), Ordering::Relaxed);
    }
}

impl Default for FixedClock {
    fn default() -> Self {
        Self::at(Self::DEFAULT)
    }
}

impl Clock for FixedClock {
    fn now(&self) -> Timestamp {
        Timestamp::from_unix_micros(self.now.load(Ordering::Relaxed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stands_still_until_set() {
        let clock = FixedClock::default();
        assert_eq!(clock.now(), FixedClock::DEFAULT);
        assert_eq!(clock.now(), FixedClock::DEFAULT);

        let later = Timestamp::from_unix_micros(FixedClock::DEFAULT.unix_micros() + 1);
        clock.set(later);
        assert_eq!(clock.now(), later);
    }
}
