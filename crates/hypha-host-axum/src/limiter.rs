//! The in-memory rate limiter: counts in this process with `governor` (GCRA, a smooth token
//! bucket), right for one instance (0006's model A on a VPS). The rules and quotas are
//! `hypha_http::rate_limit`'s.

use std::num::NonZeroU32;

use governor::{
    Quota, RateLimiter as Governor,
    clock::{Clock, DefaultClock},
    middleware::NoOpMiddleware,
    state::keyed::DashMapStateStore,
};
use hypha_config::RateLimits;
use hypha_http::rate_limit::{Decision, RateLimiter, Rule};

fn governor_quota(rule: Rule, limits: &RateLimits) -> Quota {
    let (burst, every) = rule.quota(limits);
    // The configuration's checks keep every period and burst above zero
    // (`hypha_config::Config::load`).
    Quota::with_period(every)
        .and_then(|q| NonZeroU32::new(burst).map(|b| q.allow_burst(b)))
        .unwrap_or_else(|| panic!("invalid quota for {rule:?}"))
}

type Keyed<C> =
    Governor<String, DashMapStateStore<String>, C, NoOpMiddleware<<C as Clock>::Instant>>;

/// Counts in this process: right for one instance (0006's model A on a VPS). Its keyed
/// state grows with every new key, so `forget_idle` must run every so often (the host
/// does it once a minute); otherwise rotating addresses would grow memory without bound.
pub struct MemoryRateLimiter<C: Clock = DefaultClock> {
    clock: C,
    /// One per rule, at `rule as usize`.
    limiters: [Keyed<C>; Rule::ALL.len()],
}

impl MemoryRateLimiter {
    /// With `limits`' quotas, on the real clock.
    pub fn new(limits: &RateLimits) -> Self {
        Self::with_clock(limits, DefaultClock::default())
    }
}

impl<C: Clock + Clone> MemoryRateLimiter<C> {
    /// With another clock: tests use `governor::clock::FakeRelativeClock` and step it.
    pub fn with_clock(limits: &RateLimits, clock: C) -> Self {
        let limiters = Rule::ALL
            .map(|rule| Governor::dashmap_with_clock(governor_quota(rule, limits), clock.clone()));
        Self { clock, limiters }
    }

    fn limiter(&self, rule: Rule) -> &Keyed<C> {
        &self.limiters[rule as usize]
    }

    /// Drops the keys whose state is back to fresh, and frees the memory they held.
    pub fn forget_idle(&self) {
        for limiter in &self.limiters {
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
        (
            MemoryRateLimiter::with_clock(&RateLimits::default(), clock.clone()),
            clock,
        )
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
    fn all_lists_every_rule_at_its_index() {
        for (i, rule) in Rule::ALL.into_iter().enumerate() {
            assert_eq!(rule as usize, i, "{rule:?}");
        }
    }

    #[test]
    fn every_rule_has_a_valid_default_quota() {
        for rule in Rule::ALL {
            let _ = governor_quota(rule, &RateLimits::default());
        }
    }

    #[test]
    fn the_burst_passes_then_one_more_per_period() {
        let (l, clock) = limiter();
        let (burst, every) = Rule::ResetIp.quota(&RateLimits::default());
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
        let (burst, _) = Rule::ResetEmailFromIp.quota(&RateLimits::default());
        for _ in 0..burst {
            check(&l, Rule::ResetEmailFromIp, "ana@example.com");
        }
        assert!(matches!(
            check(&l, Rule::ResetEmailFromIp, "ana@example.com"),
            Decision::Limited { .. }
        ));
        assert_eq!(
            check(&l, Rule::ResetEmailFromIp, "bob@example.com"),
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
        let (burst, every) = Rule::LoginIp.quota(&RateLimits::default());
        clock.advance(every * burst);
        l.forget_idle();
        assert_eq!(l.tracked(Rule::LoginIp), 0);
    }
}
