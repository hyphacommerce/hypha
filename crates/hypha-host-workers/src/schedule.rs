//! Which jobs a cron tick runs. The Worker has one trigger, every minute (`TICK`,
//! `wrangler.toml`), and each job runs on the ticks that start its interval
//! (`jobs.<job>.interval`, as natively), so no cron string is matched: prune, hourly by
//! default, runs at :00. A job whose interval is shorter than a tick runs every tick.
//! Portable, so its tests run natively.

use std::time::Duration;

/// How often the cron trigger fires.
pub const TICK: Duration = Duration::from_secs(60);

/// Whether the tick scheduled at `at_millis` (since the epoch) starts a new `interval`.
pub fn due(at_millis: u64, interval: Duration) -> bool {
    let tick = TICK.as_millis() as u64;
    let interval = u64::try_from(interval.as_millis())
        .unwrap_or(u64::MAX)
        .max(1);
    interval <= tick || at_millis / interval != at_millis.saturating_sub(tick) / interval
}

/// Whether a request may have moved a price list's window boundary (a list or a catalog
/// written, or a write an agent or a proposal's approval made): its after-write run then
/// writes the `due` mark again, which counts the next boundary. Other writes leave the mark
/// as the last tick found it; one that slips past (a spore's route) waits at most for the
/// safety net's tick.
pub fn may_move_boundaries(method: &str, path: &str) -> bool {
    !matches!(method, "GET" | "HEAD" | "OPTIONS")
        && [
            "/admin/price-lists",
            "/admin/catalogs",
            "/admin/markets",
            "/admin/agent-proposals",
            "/mcp",
        ]
        .iter()
        .any(|prefix| {
            path.strip_prefix(prefix)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_to_price_lists_and_catalogs_may_move_boundaries() {
        assert!(may_move_boundaries("PATCH", "/admin/price-lists/pl_1"));
        assert!(may_move_boundaries("POST", "/admin/price-lists"));
        assert!(may_move_boundaries(
            "PUT",
            "/admin/catalogs/cat_1/price-lists"
        ));
        assert!(may_move_boundaries("POST", "/mcp"));
        assert!(!may_move_boundaries("GET", "/admin/price-lists"));
        assert!(!may_move_boundaries("POST", "/admin/products"));
        assert!(!may_move_boundaries("POST", "/admin/price-listsx"));
    }

    const MINUTE: u64 = 60_000;
    const HOUR: Duration = Duration::from_secs(3600);

    #[test]
    fn an_hourly_job_runs_on_the_hours_first_tick() {
        let noon = 12 * 60 * MINUTE;
        assert!(due(noon, HOUR));
        assert!(!due(noon + MINUTE, HOUR));
        assert!(!due(noon - MINUTE, HOUR));
    }

    #[test]
    fn a_late_tick_still_counts_for_its_hour() {
        let noon = 12 * 60 * MINUTE;
        assert!(due(noon + 30_000, HOUR));
        assert!(!due(noon + MINUTE + 30_000, HOUR));
    }

    #[test]
    fn a_job_shorter_than_a_tick_runs_every_tick() {
        assert!(due(7 * MINUTE + 1, Duration::from_secs(2)));
        assert!(due(7 * MINUTE, TICK));
    }
}
