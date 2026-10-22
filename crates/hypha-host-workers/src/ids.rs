//! UUIDv7 IDs from the runtime's clock and randomness. Within one millisecond the IDs an
//! isolate makes still sort in the order they were made (the 12 bits after the time are a
//! counter, RFC 9562's method 1), as `Uuid::now_v7` keeps them natively: lists are newest
//! first by ID. Portable, so its tests run natively.

use std::sync::Mutex;

use uuid::{Builder, Uuid};

/// The largest value of the 12-bit counter.
const COUNTER_MAX: u16 = 0x0fff;

/// The last ID's time and counter.
#[derive(Default)]
pub struct V7State(Mutex<(u64, u16)>);

impl V7State {
    /// The next ID at `millis` since the epoch, from 10 random bytes.
    pub fn next(&self, millis: u64, mut random: [u8; 10]) -> Uuid {
        let mut last = self.0.lock().unwrap_or_else(|p| p.into_inner());
        let (last_millis, last_counter) = *last;
        // A clock that goes back keeps the last time, so IDs never sort backwards.
        let (millis, counter) = if millis > last_millis {
            // A fresh random start in the lower half leaves room to count up.
            (millis, u16::from_be_bytes([random[0], random[1]]) & 0x07ff)
        } else if last_counter < COUNTER_MAX {
            (last_millis, last_counter + 1)
        } else {
            (last_millis + 1, 0)
        };
        *last = (millis, counter);
        [random[0], random[1]] = counter.to_be_bytes();
        Builder::from_unix_timestamp_millis(millis, &random).into_uuid()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_in_one_millisecond_sort_in_order() {
        let state = V7State::default();
        let a = state.next(1_000, [0xff; 10]);
        let b = state.next(1_000, [0x00; 10]);
        let c = state.next(1_000, [0x80; 10]);
        assert!(a < b && b < c);
        assert_eq!(a.get_version_num(), 7);
    }

    #[test]
    fn a_later_millisecond_sorts_after() {
        let state = V7State::default();
        let a = state.next(1_000, [0x07; 10]);
        let b = state.next(1_001, [0x00; 10]);
        assert!(a < b);
    }

    #[test]
    fn a_clock_set_back_keeps_the_order() {
        let state = V7State::default();
        let a = state.next(2_000, [0x01; 10]);
        let b = state.next(1_000, [0x01; 10]);
        assert!(a < b);
    }

    #[test]
    fn a_full_counter_moves_to_the_next_millisecond() {
        let state = V7State::default();
        *state.0.lock().unwrap() = (1_000, COUNTER_MAX);
        let a = state.next(1_000, [0; 10]);
        let b = state.next(1_000, [0; 10]);
        assert!(a < b);
        assert_eq!(*state.0.lock().unwrap(), (1_001, 1));
    }
}
