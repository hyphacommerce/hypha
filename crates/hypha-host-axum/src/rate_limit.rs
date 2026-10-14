//! Rate limits on the routes that need no credentials (design note 0009).
//!
//! `RateLimiter` is the port: one `check` per rule and key. `MemoryRateLimiter` keeps the
//! counts in this process with `governor` (GCRA, a smooth token bucket); a shared
//! version (Postgres, the Workers binding) comes with the first host that runs more than
//! one instance.

use std::{num::NonZeroU32, time::Duration};

use governor::{
    Quota, RateLimiter as Governor,
    clock::{Clock, DefaultClock},
    middleware::NoOpMiddleware,
    state::keyed::DashMapStateStore,
};

/// Every limit, with its quota in `quota`: a closed list, so a handler can't invent a
/// limit and all of them are in one place (0009, section 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Rule {
    /// `POST /auth/login`, per client IP.
    LoginIp,
    /// `POST /auth/login`, per email: bounds guesses at one account from any number of
    /// addresses.
    LoginEmail,
    /// `POST /auth/login/mfa`, per client IP. Per account, the second-factor lockout
    /// already counts (0008).
    MfaIp,
    /// `POST /auth/password-reset` and `/confirm`, per client IP.
    ResetIp,
    /// `POST /auth/password-reset`, per email. The domain also sends at most one email
    /// per account every few minutes; this stops the work before that.
    ResetEmail,
    /// `POST /invites/accept`, per client IP.
    InviteIp,
}

impl Rule {
    pub const ALL: [Rule; 6] = [
        Rule::LoginIp,
        Rule::LoginEmail,
        Rule::MfaIp,
        Rule::ResetIp,
        Rule::ResetEmail,
        Rule::InviteIp,
    ];

    /// `(burst, one more every)`: the starting numbers from 0009, section 3.
    pub fn quota(self) -> (u32, Duration) {
        match self {
            Rule::LoginIp | Rule::LoginEmail | Rule::MfaIp | Rule::InviteIp => {
                (10, Duration::from_secs(30))
            }
            Rule::ResetIp => (5, Duration::from_secs(60)),
            Rule::ResetEmail => (3, Duration::from_secs(5 * 60)),
        }
    }

    fn governor_quota(self) -> Quota {
        let (burst, every) = self.quota();
        // Every quota above has a non-zero period and burst; a zero would be a typo in
        // this file, caught by the tests.
        Quota::with_period(every)
            .and_then(|q| NonZeroU32::new(burst).map(|b| q.allow_burst(b)))
            .unwrap_or_else(|| panic!("invalid quota for {self:?}"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allowed,
    /// Over the limit; the next attempt is allowed after `retry_after`.
    Limited {
        retry_after: Duration,
    },
}

/// `Err(429 rate_limited)` when `key` is over `rule`'s limit: what handlers call.
pub async fn enforce(
    limiter: &impl RateLimiter,
    rule: Rule,
    key: &str,
) -> Result<(), crate::error::ApiError> {
    match limiter.check(rule, key).await {
        Decision::Allowed => Ok(()),
        Decision::Limited { retry_after } => {
            tracing::info!(?rule, "rate limited");
            Err(crate::error::ApiError::rate_limited(retry_after))
        }
    }
}

/// The port. Async, because a shared version waits on the network.
#[allow(async_fn_in_trait)]
pub trait RateLimiter {
    async fn check(&self, rule: Rule, key: &str) -> Decision;
}

type Keyed<C> =
    Governor<String, DashMapStateStore<String>, C, NoOpMiddleware<<C as Clock>::Instant>>;

/// Counts in this process: right for one instance (0006's model A on a VPS). Its keyed
/// state grows with every new key, so `forget_idle` must run every so often (the host
/// does it once a minute); otherwise rotating addresses would grow memory without bound.
pub struct MemoryRateLimiter<C: Clock = DefaultClock> {
    clock: C,
    limiters: Vec<(Rule, Keyed<C>)>,
}

impl Default for MemoryRateLimiter {
    fn default() -> Self {
        Self::with_clock(DefaultClock::default())
    }
}

impl<C: Clock + Clone> MemoryRateLimiter<C> {
    /// With another clock: tests use `governor::clock::FakeRelativeClock` and step it.
    pub fn with_clock(clock: C) -> Self {
        let limiters = Rule::ALL
            .into_iter()
            .map(|rule| {
                (
                    rule,
                    Governor::dashmap_with_clock(rule.governor_quota(), clock.clone()),
                )
            })
            .collect();
        Self { clock, limiters }
    }

    fn limiter(&self, rule: Rule) -> &Keyed<C> {
        // `with_clock` makes one per rule in `Rule::ALL`, so this always finds one.
        &self
            .limiters
            .iter()
            .find(|(r, _)| *r == rule)
            .unwrap_or_else(|| panic!("no limiter for {rule:?}"))
            .1
    }

    /// Drops the keys whose state is back to fresh, and frees the memory they held.
    pub fn forget_idle(&self) {
        for (_, limiter) in &self.limiters {
            limiter.retain_recent();
            limiter.shrink_to_fit();
        }
    }

    /// How many keys `rule` is tracking.
    #[cfg(test)]
    pub fn tracked(&self, rule: Rule) -> usize {
        self.limiter(rule).len()
    }
}

impl<C: Clock + Clone> RateLimiter for MemoryRateLimiter<C> {
    async fn check(&self, rule: Rule, key: &str) -> Decision {
        match self.limiter(rule).check_key(&key.to_string()) {
            Ok(()) => Decision::Allowed,
            Err(not_until) => Decision::Limited {
                retry_after: not_until.wait_time_from(self.clock.now()),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use governor::clock::FakeRelativeClock;

    use super::*;

    fn limiter() -> (MemoryRateLimiter<FakeRelativeClock>, FakeRelativeClock) {
        let clock = FakeRelativeClock::default();
        (MemoryRateLimiter::with_clock(clock.clone()), clock)
    }

    /// The in-memory limiter never awaits anything, so one poll finishes a check.
    fn check(l: &MemoryRateLimiter<FakeRelativeClock>, rule: Rule, key: &str) -> Decision {
        let mut future = std::pin::pin!(l.check(rule, key));
        let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
        match future.as_mut().poll(&mut cx) {
            std::task::Poll::Ready(decision) => decision,
            std::task::Poll::Pending => unreachable!("the in-memory limiter never waits"),
        }
    }

    #[test]
    fn every_rule_has_a_valid_quota() {
        for rule in Rule::ALL {
            let _ = rule.governor_quota();
        }
    }

    #[test]
    fn the_burst_passes_then_one_more_per_period() {
        let (l, clock) = limiter();
        let (burst, every) = Rule::ResetIp.quota();
        for _ in 0..burst {
            assert_eq!(check(&l, Rule::ResetIp, "1.2.3.4"), Decision::Allowed);
        }
        assert_eq!(
            check(&l, Rule::ResetIp, "1.2.3.4"),
            Decision::Limited { retry_after: every }
        );
        clock.advance(every);
        assert_eq!(check(&l, Rule::ResetIp, "1.2.3.4"), Decision::Allowed);
        assert!(matches!(
            check(&l, Rule::ResetIp, "1.2.3.4"),
            Decision::Limited { .. }
        ));
    }

    #[test]
    fn keys_and_rules_count_separately() {
        let (l, _) = limiter();
        let (burst, _) = Rule::ResetEmail.quota();
        for _ in 0..burst {
            check(&l, Rule::ResetEmail, "ana@example.com");
        }
        assert!(matches!(
            check(&l, Rule::ResetEmail, "ana@example.com"),
            Decision::Limited { .. }
        ));
        assert_eq!(
            check(&l, Rule::ResetEmail, "bob@example.com"),
            Decision::Allowed
        );
        assert_eq!(
            check(&l, Rule::LoginEmail, "ana@example.com"),
            Decision::Allowed
        );
    }

    #[test]
    fn idle_keys_are_forgotten() {
        let (l, clock) = limiter();
        for n in 0..100 {
            check(&l, Rule::LoginIp, &format!("10.0.0.{n}"));
        }
        assert_eq!(l.tracked(Rule::LoginIp), 100);
        // Long enough for every key's state to be back to fresh.
        let (burst, every) = Rule::LoginIp.quota();
        clock.advance(every * burst);
        l.forget_idle();
        assert_eq!(l.tracked(Rule::LoginIp), 0);
    }
}
