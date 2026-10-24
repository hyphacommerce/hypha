//! Windows (an optional start and end) whose boundaries a schedule job announces: which
//! passed in a stretch of time, and when the next one is. What a window belongs to, and
//! whether it counts at all, is the caller's.

use crate::Timestamp;

/// Which end of a window passed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Boundary {
    Started,
    Ended,
}

/// What a run of a schedule job did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Announced {
    /// Events recorded.
    pub events: u32,
    /// The next boundary after now, for a host that runs jobs only when they're due.
    pub next: Option<Timestamp>,
}

/// The boundaries of `windows` (`(owner, starts_at, ends_at)`) that passed in
/// `(after, until]`, oldest first (then by owner, a start before an end).
pub fn passed<I: Copy + Ord>(
    windows: impl IntoIterator<Item = (I, Option<Timestamp>, Option<Timestamp>)>,
    after: Timestamp,
    until: Timestamp,
) -> Vec<(I, Boundary, Timestamp)> {
    let mut found: Vec<(I, Boundary, Timestamp)> = windows
        .into_iter()
        .flat_map(|(owner, starts, ends)| {
            [(Boundary::Started, starts), (Boundary::Ended, ends)]
                .into_iter()
                .filter_map(move |(boundary, at)| at.map(|at| (owner, boundary, at)))
        })
        .filter(|(_, _, at)| *at > after && *at <= until)
        .collect();
    found.sort_by_key(|(owner, boundary, at)| (*at, *owner, *boundary));
    found
}

/// The first boundary of `windows` after `after`.
pub fn next(
    windows: impl IntoIterator<Item = (Option<Timestamp>, Option<Timestamp>)>,
    after: Timestamp,
) -> Option<Timestamp> {
    windows
        .into_iter()
        .flat_map(|(starts, ends)| [starts, ends])
        .flatten()
        .filter(|at| *at > after)
        .min()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(micros: i64) -> Timestamp {
        Timestamp::from_unix_micros(micros)
    }

    #[test]
    fn a_boundary_inside_the_stretch_passed() {
        assert_eq!(
            passed([(1, Some(at(100)), None)], at(50), at(150)),
            vec![(1, Boundary::Started, at(100))]
        );
    }

    #[test]
    fn a_boundary_at_the_stretchs_start_was_the_last_runs() {
        assert_eq!(passed([(1, Some(at(100)), None)], at(100), at(150)), vec![]);
    }

    #[test]
    fn a_boundary_at_the_stretchs_end_counts() {
        assert_eq!(
            passed([(1, None, Some(at(150)))], at(100), at(150)),
            vec![(1, Boundary::Ended, at(150))]
        );
    }

    #[test]
    fn a_boundary_still_ahead_waits() {
        assert_eq!(passed([(1, Some(at(151)), None)], at(100), at(150)), vec![]);
    }

    #[test]
    fn boundaries_come_oldest_first_then_by_owner() {
        assert_eq!(
            passed(
                [(2, Some(at(110)), Some(at(140))), (1, Some(at(120)), None)],
                at(100),
                at(150)
            ),
            vec![
                (2, Boundary::Started, at(110)),
                (1, Boundary::Started, at(120)),
                (2, Boundary::Ended, at(140)),
            ]
        );
    }

    #[test]
    fn the_next_boundary_is_the_earliest_ahead() {
        let windows = [(Some(at(100)), Some(at(300))), (Some(at(200)), None)];
        assert_eq!(next(windows, at(150)), Some(at(200)));
        assert_eq!(next(windows, at(300)), None);
    }
}
