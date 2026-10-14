//! Points in time. Core and domain crates never read the system clock (it panics on
//! `wasm32-unknown-unknown`, and tests need exact times), so "now" comes in through the
//! `Clock` port (`ports.rs`).

/// A point in time: microseconds since the Unix epoch, in UTC.
///
/// Microseconds because Postgres `timestamptz` stores exactly that, so a timestamp
/// reads back equal to what was written. There are no time zones or calendar rules
/// here; hosts format RFC 3339 at the edges. Every `i64` is accepted, though Postgres
/// only stores years 4713 BC to 294276 AD; a clock never gets near either end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(i64);

impl Timestamp {
    pub const UNIX_EPOCH: Timestamp = Timestamp(0);

    pub const fn from_unix_micros(micros: i64) -> Self {
        Self(micros)
    }

    pub const fn unix_micros(self) -> i64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn later_timestamps_compare_greater() {
        let earlier = Timestamp::from_unix_micros(-1);
        assert!(earlier < Timestamp::UNIX_EPOCH);
        assert!(Timestamp::UNIX_EPOCH < Timestamp::from_unix_micros(1));
    }

    #[test]
    fn micros_round_trip() {
        assert_eq!(
            Timestamp::from_unix_micros(1_767_225_600_000_000).unix_micros(),
            1_767_225_600_000_000
        );
    }
}
