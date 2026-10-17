//! The rate limiter for now (design note 0009's rules and quotas): GCRA counted in this
//! isolate's memory. A Worker runs in many isolates, each counting on its own, so a
//! client spread over them gets more than its quota; Cloudflare's Rate Limiting binding
//! replaces this (0026, W8). Portable, so its tests run natively.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use hypha_config::RateLimits;
use hypha_core::Timestamp;
use hypha_http::rate_limit::{Decision, Rule};

/// Keys tracked per rule before the ones back to fresh are dropped. If too many are
/// still counting (someone rotating addresses), the half nearest to fresh go too: their
/// next tries are allowed sooner than they should be, but memory and the time a check
/// takes stay bounded, and the busiest keys (an attacker's own) keep their counts.
const PRUNE_ABOVE: usize = 10_000;

/// GCRA (a smooth token bucket, as `governor` counts natively): per key, the time its
/// bucket is next empty (the "theoretical arrival time"), in µs since the epoch.
pub struct IsolateLimiter {
    /// `(burst, one more every)` per rule, at `rule as usize`.
    quotas: [(u32, Duration); Rule::ALL.len()],
    keys: Mutex<[HashMap<String, i64>; Rule::ALL.len()]>,
}

impl IsolateLimiter {
    pub fn new(limits: &RateLimits) -> Self {
        Self {
            quotas: Rule::ALL.map(|rule| rule.quota(limits)),
            keys: Mutex::new(std::array::from_fn(|_| HashMap::new())),
        }
    }

    /// Counts one attempt by `key` under `rule` at `now`.
    pub fn check(&self, rule: Rule, key: &str, now: Timestamp) -> Decision {
        let (burst, every) = self.quotas[rule as usize];
        let every = i64::try_from(every.as_micros()).unwrap_or(i64::MAX);
        let now = now.unix_micros();
        let mut keys = self.keys.lock().unwrap_or_else(|p| p.into_inner());
        let keys = &mut keys[rule as usize];
        if keys.len() > PRUNE_ABOVE {
            keys.retain(|_, empty_at| *empty_at > now);
        }
        if keys.len() > PRUNE_ABOVE {
            let mut times: Vec<i64> = keys.values().copied().collect();
            let (_, &mut keep_from, _) = times.select_nth_unstable(PRUNE_ABOVE / 2);
            keys.retain(|_, empty_at| *empty_at > keep_from);
        }
        let empty_at = keys.get(key).copied().unwrap_or(now).max(now);
        let next = empty_at.saturating_add(every);
        // Allowed while the bucket holds less than `burst` attempts' worth.
        let allowed_from = next.saturating_sub(every.saturating_mul(i64::from(burst)));
        if now < allowed_from {
            let wait = u64::try_from(allowed_from - now).unwrap_or(0);
            return Decision::Limited {
                retry_after: Duration::from_micros(wait),
            };
        }
        keys.insert(key.to_string(), next);
        Decision::Allowed
    }

    /// How many keys `rule` is tracking.
    #[cfg(test)]
    fn tracked(&self, rule: Rule) -> usize {
        self.keys.lock().unwrap()[rule as usize].len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEC: i64 = 1_000_000;

    fn at(secs: i64) -> Timestamp {
        Timestamp::from_unix_micros(1_700_000_000 * SEC + secs * SEC)
    }

    fn limiter() -> (IsolateLimiter, u32, i64) {
        let limits = RateLimits::default();
        let (burst, every) = Rule::LoginIp.quota(&limits);
        let every = i64::try_from(every.as_micros()).unwrap() / SEC;
        (IsolateLimiter::new(&limits), burst, every)
    }

    #[test]
    fn a_burst_is_allowed_then_limited() {
        let (limiter, burst, every) = limiter();
        for _ in 0..burst {
            assert_eq!(limiter.check(Rule::LoginIp, "a", at(0)), Decision::Allowed);
        }
        assert_eq!(
            limiter.check(Rule::LoginIp, "a", at(0)),
            Decision::Limited {
                retry_after: Duration::from_secs(every as u64)
            }
        );
    }

    #[test]
    fn one_more_is_allowed_after_each_period() {
        let (limiter, burst, every) = limiter();
        for _ in 0..burst {
            limiter.check(Rule::LoginIp, "a", at(0));
        }
        assert!(matches!(
            limiter.check(Rule::LoginIp, "a", at(every - 1)),
            Decision::Limited { .. }
        ));
        assert_eq!(
            limiter.check(Rule::LoginIp, "a", at(every)),
            Decision::Allowed
        );
        assert!(matches!(
            limiter.check(Rule::LoginIp, "a", at(every)),
            Decision::Limited { .. }
        ));
    }

    #[test]
    fn keys_and_rules_count_apart() {
        let (limiter, burst, _) = limiter();
        for _ in 0..burst {
            limiter.check(Rule::LoginIp, "a", at(0));
        }
        assert_eq!(limiter.check(Rule::LoginIp, "b", at(0)), Decision::Allowed);
        assert_eq!(limiter.check(Rule::MfaIp, "a", at(0)), Decision::Allowed);
    }

    #[test]
    fn too_many_counting_keys_drop_the_half_nearest_to_fresh() {
        let (limiter, burst, _) = limiter();
        // One busy key, then many that each tried once.
        for _ in 0..burst {
            limiter.check(Rule::LoginIp, "busy", at(0));
        }
        for n in 0..PRUNE_ABOVE {
            limiter.check(Rule::LoginIp, &n.to_string(), at(0));
        }
        limiter.check(Rule::LoginIp, "one more", at(0));
        assert!(limiter.tracked(Rule::LoginIp) <= PRUNE_ABOVE / 2 + 1);
        assert!(matches!(
            limiter.check(Rule::LoginIp, "busy", at(0)),
            Decision::Limited { .. }
        ));
    }

    #[test]
    fn fresh_keys_are_dropped_once_many_are_tracked() {
        let (limiter, _, every) = limiter();
        for n in 0..=PRUNE_ABOVE {
            limiter.check(Rule::LoginIp, &n.to_string(), at(0));
        }
        assert_eq!(limiter.tracked(Rule::LoginIp), PRUNE_ABOVE + 1);
        limiter.check(Rule::LoginIp, "late", at(every));
        assert_eq!(limiter.tracked(Rule::LoginIp), 1);
    }
}
