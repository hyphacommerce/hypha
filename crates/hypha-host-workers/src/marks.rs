//! Whether a cron tick has work, read from KV so that a quiet store's ticks don't touch
//! Postgres and a serverless database can sleep.
//!
//! Three marks, each a time in milliseconds since the epoch, so no write undoes
//! another's (KV has no compare-and-set, and the last write wins):
//!
//! - `written`: a run (after a write, or a tick's) ended with work left;
//! - `checked`: the start of the last tick whose run left nothing;
//! - `due`: when work is next due: the dispatcher's (`OutboxRepo::next_due`: a retry,
//!   the end of a lease), the send job's (`NotificationRepo::next_due`: a queued email's
//!   next attempt, the end of a lease) or a failed search rebuild's (its lease's end), as
//!   the last run that did something found it.
//!
//! A tick runs the jobs when a run left work since the last clean check, when `due` came
//! between that check and now, or when nothing was ever checked (a new namespace: setup may
//! have queued a search rebuild). After a write, its own run usually does the work, so most
//! writes set nothing. What a lost mark could hide (two runs racing to write `due`, a run
//! killed before it marked) waits at most for the safety net, a run on the first tick of
//! every `NET`. Portable, so its tests run natively.

use std::time::Duration;

/// The KV keys.
pub const WRITTEN: &str = "jobs:written";
pub const CHECKED: &str = "jobs:checked";
pub const DUE: &str = "jobs:due";
/// Set while prune or the media sweep has work left: the next tick goes on with it.
pub const BACKLOG: &str = "jobs:backlog";

/// How often a tick runs the jobs whatever the marks say.
pub const NET: Duration = Duration::from_secs(3600);

/// Room for the clocks of different isolates to disagree, in milliseconds: a mark this
/// much older than a check still counts.
const SKEW: u64 = 5_000;

/// The marks as a tick reads them; `None` for one never written (or unreadable).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Marks {
    pub written: Option<u64>,
    pub checked: Option<u64>,
    pub due: Option<u64>,
}

impl Marks {
    /// The marks from KV's values.
    pub fn parse(written: Option<&str>, checked: Option<&str>, due: Option<&str>) -> Self {
        let time = |v: Option<&str>| v.and_then(|v| v.trim().parse().ok());
        Self {
            written: time(written),
            checked: time(checked),
            due: time(due),
        }
    }

    /// Whether a tick at `now` has work waiting.
    pub fn waiting(&self, now: u64) -> bool {
        let Some(checked) = self.checked else {
            return true;
        };
        let since_check = |at: u64| at + SKEW > checked;
        self.written.is_some_and(since_check)
            || self.due.is_some_and(|due| since_check(due) && due <= now)
    }
}

/// How the jobs' runs in one event went.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Run {
    /// A step stopped with more to do (its step limit or deadline), or failed.
    pub left: bool,
    /// A step found something to do.
    pub worked: bool,
    /// The earliest time work a step couldn't finish is due again (a retry, a delivery or
    /// search rebuild left under its lease), in milliseconds.
    pub retry_at: Option<u64>,
}

impl Run {
    pub fn and(self, other: Run) -> Run {
        Run {
            left: self.left || other.left,
            worked: self.worked || other.worked,
            retry_at: earliest(self.retry_at, other.retry_at),
        }
    }
}

/// The earlier of two times, either maybe unset.
pub fn earliest(a: Option<u64>, b: Option<u64>) -> Option<u64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

/// What to write after a run: `started` is the tick's start for a tick's run (`None`
/// after a write).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct After {
    /// Write `written` (now).
    pub written: bool,
    /// Write `checked` with this time.
    pub checked: Option<u64>,
    /// Write `due`: the earliest of the outbox's and the emails' next due times and
    /// `retry_at`.
    pub due: bool,
    pub retry_at: Option<u64>,
}

pub fn after(run: Run, started: Option<u64>) -> After {
    After {
        written: run.left,
        checked: started.filter(|_| !run.left),
        // A tick's run checks it each time: it's the one that keeps it right. After a write,
        // only a run that left something (now or for later) does: most leave nothing.
        due: run.left || run.retry_at.is_some() || started.is_some(),
        retry_at: run.retry_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_800_000_000_000;
    const MIN: u64 = 60_000;

    fn marks(written: Option<u64>, checked: Option<u64>, due: Option<u64>) -> Marks {
        Marks {
            written,
            checked,
            due,
        }
    }

    #[test]
    fn nothing_ever_checked_is_work() {
        assert!(Marks::default().waiting(NOW));
        assert!(marks(None, None, Some(NOW + MIN)).waiting(NOW));
    }

    #[test]
    fn a_quiet_store_has_none() {
        assert!(!marks(None, Some(NOW - MIN), None).waiting(NOW));
        assert!(!marks(Some(NOW - 2 * MIN), Some(NOW - MIN), Some(NOW - 3 * MIN)).waiting(NOW));
    }

    #[test]
    fn work_left_after_the_check_is_work() {
        assert!(marks(Some(NOW - 1), Some(NOW - MIN), None).waiting(NOW));
    }

    #[test]
    fn a_mark_within_the_skew_before_the_check_still_counts() {
        let checked = NOW - MIN;
        assert!(marks(Some(checked - SKEW + 1), Some(checked), None).waiting(NOW));
        assert!(!marks(Some(checked - SKEW), Some(checked), None).waiting(NOW));
    }

    #[test]
    fn a_retry_counts_once_due() {
        let m = marks(None, Some(NOW - MIN), Some(NOW + 1));
        assert!(!m.waiting(NOW));
        assert!(m.waiting(NOW + 1));
    }

    #[test]
    fn a_retry_handled_by_the_check_does_not() {
        assert!(!marks(None, Some(NOW - MIN), Some(NOW - MIN - SKEW)).waiting(NOW));
    }

    #[test]
    fn unreadable_marks_are_unwritten() {
        assert_eq!(
            Marks::parse(Some("12"), Some("soon"), None),
            marks(Some(12), None, None)
        );
    }

    #[test]
    fn a_clean_run_after_a_write_writes_nothing() {
        let after = after(Run::default(), None);
        assert_eq!(
            after,
            After {
                written: false,
                checked: None,
                due: false,
                retry_at: None,
            }
        );
    }

    #[test]
    fn a_run_that_did_all_its_work_after_a_write_reads_nothing() {
        let run = Run {
            worked: true,
            ..Run::default()
        };
        assert!(!after(run, None).due);
        assert!(!after(run, None).written);
    }

    #[test]
    fn a_run_with_work_left_marks_it_and_does_not_check() {
        let run = Run {
            left: true,
            worked: true,
            retry_at: None,
        };
        assert_eq!(
            after(run, Some(NOW)),
            After {
                written: true,
                checked: None,
                due: true,
                retry_at: None,
            }
        );
    }

    #[test]
    fn a_failed_rebuild_writes_when_it_is_due_again() {
        let run = Run {
            worked: true,
            retry_at: Some(NOW + 2 * MIN),
            ..Run::default()
        };
        let after = after(run, None);
        assert!(after.due && !after.written);
        assert_eq!(after.retry_at, Some(NOW + 2 * MIN));
    }

    #[test]
    fn runs_together_keep_the_earliest_retry() {
        let at = |t| Run {
            retry_at: t,
            ..Run::default()
        };
        assert_eq!(at(Some(5)).and(at(Some(3))).retry_at, Some(3));
        assert_eq!(at(None).and(at(Some(3))).retry_at, Some(3));
        assert_eq!(at(None).and(at(None)).retry_at, None);
    }

    #[test]
    fn a_clean_tick_checks_from_its_start() {
        assert_eq!(after(Run::default(), Some(NOW)).checked, Some(NOW));
        assert!(after(Run::default(), Some(NOW)).due);
    }
}
