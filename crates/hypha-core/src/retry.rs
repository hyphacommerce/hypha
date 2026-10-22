//! Trying a write again when another request committed first. Versioned writes fail with
//! `RepoError::StaleVersion` (or a unique key the racing request took) instead of
//! overwriting; a command that can redo its work on fresh data does so a few times before
//! reporting the conflict.

/// How many times a command tries its write in total before a lost race is reported.
pub const MAX_ATTEMPTS: u32 = 3;

/// Counts a command's attempts, so every retry loop stops at `MAX_ATTEMPTS`.
///
/// `retry_on_conflict` uses it; a loop that has to do more between attempts than the
/// helper allows (checkout looks for an idempotent replay and keeps a renewed quote)
/// counts with it directly.
#[derive(Debug)]
pub struct Attempts {
    tried: u32,
}

impl Attempts {
    /// The first attempt is under way.
    pub fn first() -> Self {
        Self { tried: 1 }
    }

    /// Whether another attempt is allowed; if so, it counts as started.
    pub fn another(&mut self) -> bool {
        if self.tried < MAX_ATTEMPTS {
            self.tried += 1;
            true
        } else {
            false
        }
    }
}

/// Runs `attempt` again (it re-reads everything it needs) while its error is one
/// `lost_a_race` accepts, up to `MAX_ATTEMPTS` times in total. Any other result,
/// including a real problem found on fresh data, is returned as is.
///
/// Takes a closure returning a future rather than an `async` closure (`AsyncFn`): the
/// latter's future borrows from the closure through a higher-ranked lifetime, and the
/// compiler can't yet prove such futures `Send`, which hosts like axum need. Callers
/// move `Copy` references into each attempt instead (`move || async move { … }`).
/// No `Send` bound here either: the futures are whatever the repos return (see
/// `ports.rs`), and a host's concrete types prove it.
pub async fn retry_on_conflict<T, E, F, Fut>(
    mut attempt: F,
    lost_a_race: impl Fn(&E) -> bool,
) -> Result<T, E>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, E>>,
{
    let mut attempts = Attempts::first();
    loop {
        match attempt().await {
            Err(e) if lost_a_race(&e) && attempts.another() => {}
            result => return result,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;
    use crate::RepoError;
    use pollster::block_on;

    fn stale(e: &RepoError) -> bool {
        *e == RepoError::StaleVersion
    }

    /// An attempt that fails with each of `errors` in turn, then succeeds with the
    /// number of the attempt; `calls` counts them.
    fn failing<'a>(
        calls: &'a Cell<u32>,
        errors: &'a [RepoError],
    ) -> impl FnMut() -> std::future::Ready<Result<u32, RepoError>> + 'a {
        move || {
            let n = calls.get();
            calls.set(n + 1);
            std::future::ready(match errors.get(n as usize) {
                Some(e) => Err(e.clone()),
                None => Ok(n + 1),
            })
        }
    }

    #[test]
    fn the_first_success_is_returned() {
        let calls = Cell::new(0);
        let result = block_on(retry_on_conflict(failing(&calls, &[]), stale));
        assert_eq!(result, Ok(1));
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn a_lost_race_is_tried_again() {
        let calls = Cell::new(0);
        let errors = [RepoError::StaleVersion, RepoError::StaleVersion];
        let result = block_on(retry_on_conflict(failing(&calls, &errors), stale));
        assert_eq!(result, Ok(MAX_ATTEMPTS));
        assert_eq!(calls.get(), MAX_ATTEMPTS);
    }

    #[test]
    fn races_stop_after_max_attempts() {
        let calls = Cell::new(0);
        let errors = vec![RepoError::StaleVersion; MAX_ATTEMPTS as usize + 1];
        let result = block_on(retry_on_conflict(failing(&calls, &errors), stale));
        assert_eq!(result, Err(RepoError::StaleVersion));
        assert_eq!(calls.get(), MAX_ATTEMPTS);
    }

    #[test]
    fn another_error_stops_at_once() {
        let calls = Cell::new(0);
        let errors = [RepoError::Duplicate("handle")];
        let result = block_on(retry_on_conflict(failing(&calls, &errors), stale));
        assert_eq!(result, Err(RepoError::Duplicate("handle")));
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn attempts_allow_max_attempts_in_total() {
        let mut attempts = Attempts::first();
        for _ in 1..MAX_ATTEMPTS {
            assert!(attempts.another());
        }
        assert!(!attempts.another());
    }
}
