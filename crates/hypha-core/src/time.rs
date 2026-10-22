//! Points in time. Core and domain crates never read the system clock (it panics on
//! `wasm32-unknown-unknown`, and tests need exact times), so "now" comes in through the
//! `Clock` port (`ports.rs`).

use core::time::Duration;

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

    /// Whole seconds since the epoch, rounded down (TOTP counts 30-second steps).
    pub const fn unix_secs(self) -> i64 {
        self.0.div_euclid(1_000_000)
    }

    /// `secs` seconds later (earlier if negative), saturating at the ends of `i64`.
    pub const fn plus_secs(self, secs: i64) -> Self {
        Self(self.0.saturating_add(secs.saturating_mul(1_000_000)))
    }

    /// `duration` later, saturating at the end of `i64` (sub-microsecond parts dropped).
    /// Settings are `Duration`s: a plain number, so it builds for wasm, unlike `Instant`
    /// and `SystemTime`.
    pub const fn plus(self, duration: Duration) -> Self {
        Self(self.0.saturating_add(micros(duration)))
    }

    /// `duration` earlier, saturating at the start of `i64`.
    pub const fn minus(self, duration: Duration) -> Self {
        Self(self.0.saturating_sub(micros(duration)))
    }
}

/// A duration's whole microseconds, capped at `i64::MAX`.
const fn micros(duration: Duration) -> i64 {
    let micros = duration.as_micros();
    if micros > i64::MAX as u128 {
        i64::MAX
    } else {
        micros as i64
    }
}

/// When an aggregate was first stored and last changed.
///
/// Set by the storage adapter when it writes (or stages) the aggregate, from the clock
/// its ports carry, the same way it sets the version: an insert stamps both, an update
/// moves `updated_at`. `None` on an aggregate that was never stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamps {
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl Stamps {
    /// Created and updated `at`.
    pub fn new(at: Timestamp) -> Self {
        Self {
            created_at: at,
            updated_at: at,
        }
    }

    /// Updated `at`; created when it was.
    pub fn touched(self, at: Timestamp) -> Self {
        Self {
            updated_at: at,
            ..self
        }
    }
}

/// An aggregate that carries its `Stamps`. Domain code reads them; only adapters set
/// them, through `stamp_insert` and `stamp_update`.
pub trait Stamped {
    fn stamps(&self) -> Option<Stamps>;
    fn set_stamps(&mut self, stamps: Stamps);
}

/// Stamps an aggregate about to be inserted: created and updated `now`.
pub fn stamp_insert(aggregate: &mut impl Stamped, now: Timestamp) {
    aggregate.set_stamps(Stamps::new(now));
}

/// Stamps an aggregate about to be updated: updated `now`, created as before (or `now`,
/// for a row stored before stamps existed and loaded without them).
pub fn stamp_update(aggregate: &mut impl Stamped, now: Timestamp) {
    let stamps = aggregate
        .stamps()
        .map_or_else(|| Stamps::new(now), |s| s.touched(now));
    aggregate.set_stamps(stamps);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Thing(Option<Stamps>);

    impl Stamped for Thing {
        fn stamps(&self) -> Option<Stamps> {
            self.0
        }
        fn set_stamps(&mut self, stamps: Stamps) {
            self.0 = Some(stamps);
        }
    }

    #[test]
    fn durations_move_by_whole_microseconds_and_saturate() {
        let t = Timestamp(10);
        assert_eq!(t.plus(Duration::from_nanos(2_999)), Timestamp(12));
        assert_eq!(t.minus(Duration::from_micros(20)), Timestamp(-10));
        assert_eq!(t.plus(Duration::MAX), Timestamp(i64::MAX));
        assert_eq!(
            Timestamp(i64::MIN + 1).minus(Duration::from_secs(1)),
            Timestamp(i64::MIN)
        );
    }

    #[test]
    fn an_update_moves_only_updated_at() {
        let (one, two) = (Timestamp(1), Timestamp(2));
        let mut thing = Thing::default();
        stamp_insert(&mut thing, one);
        stamp_update(&mut thing, two);
        assert_eq!(
            thing.0,
            Some(Stamps {
                created_at: one,
                updated_at: two
            })
        );
    }

    #[test]
    fn later_timestamps_compare_greater() {
        let earlier = Timestamp::from_unix_micros(-1);
        assert!(earlier < Timestamp::UNIX_EPOCH);
        assert!(Timestamp::UNIX_EPOCH < Timestamp::from_unix_micros(1));
    }

    #[test]
    fn plus_secs_moves_by_whole_seconds_and_unix_secs_rounds_down() {
        let t = Timestamp::from_unix_micros(1_500_000);
        assert_eq!(t.plus_secs(2), Timestamp::from_unix_micros(3_500_000));
        assert_eq!(t.unix_secs(), 1);
        assert_eq!(Timestamp::from_unix_micros(-1).unix_secs(), -1);
    }

    #[test]
    fn micros_round_trip() {
        assert_eq!(
            Timestamp::from_unix_micros(1_767_225_600_000_000).unix_micros(),
            1_767_225_600_000_000
        );
    }
}
