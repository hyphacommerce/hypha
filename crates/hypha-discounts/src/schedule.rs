//! Announcing discount windows: carts follow a discount's window by themselves (they read
//! the time), but what's computed or cached ahead (storefront pages and their promotion
//! badges, apps) learns that a window opened or closed only from `discount.started` /
//! `discount.ended`, recorded here by the schedule job.

use hypha_core::{
    Clock, DiscountId, HasBatch, PendingEvent, RepoError, StageEvents, Subject, Timestamp,
    schedule::{self, Announced, Boundary},
};

use crate::{
    DiscountRepo, HasDiscounts, StageDiscounts, Window,
    events::{DISCOUNT, DiscountEnded, DiscountStarted},
};

/// The boundaries of active discounts' `windows` (a draft applies nothing either side,
/// so storage reads only active ones) that passed in `(after, until]`, oldest first (then
/// by discount, a start before an end). Each is announced even when the discount was
/// written after it: that write's `discount.updated` may have said nothing about its
/// window (a rename).
pub fn boundaries(
    windows: &[Window],
    after: Timestamp,
    until: Timestamp,
) -> Vec<(DiscountId, Boundary, Timestamp)> {
    schedule::passed(
        windows.iter().map(|w| (w.discount, w.starts_at, w.ends_at)),
        after,
        until,
    )
}

/// The first boundary of `windows` after `after`.
pub fn next_boundary(windows: &[Window], after: Timestamp) -> Option<Timestamp> {
    schedule::next(windows.iter().map(|w| (w.starts_at, w.ends_at)), after)
}

/// Records `discount.started` / `discount.ended` for every boundary that passed since the
/// last run that announced one, up to now, with the progress in the same commit. Runs may
/// race: the one that commits first announces, the other finds its progress moved
/// (`StaleVersion`) and announces nothing. The first run ever only notes where it starts.
pub async fn announce_windows<S>(ports: &S, clock: &impl Clock) -> Result<Announced, RepoError>
where
    S: HasDiscounts + HasBatch,
    S::Batch: StageEvents + StageDiscounts,
{
    let now = clock.now();
    let ran_until = ports.discounts().schedule_ran_until().await?;
    // One read covers both what passed since the last run and what's ahead.
    let from = ran_until.map_or(now, |r| r.min(now));
    let discounts = ports.discounts().with_boundary_after(from).await?;
    let next = next_boundary(&discounts, now);
    let Some(after) = ran_until else {
        let mut batch = ports.batch();
        batch.advance_discount_schedule(None, now);
        return match ports.commit(batch).await {
            Ok(()) | Err(RepoError::StaleVersion) => Ok(Announced { events: 0, next }),
            Err(e) => Err(e),
        };
    };
    let passed = boundaries(&discounts, after, now);
    if passed.is_empty() {
        return Ok(Announced { events: 0, next });
    }
    let method = |id: DiscountId| {
        discounts
            .iter()
            .find(|w| w.discount == id)
            .map_or("", |w| w.method.as_str())
    };
    let events: Vec<PendingEvent> = passed
        .iter()
        .map(|&(discount, boundary, at)| {
            let subject = Subject::new(DISCOUNT, discount.as_uuid());
            let method = method(discount);
            match boundary {
                Boundary::Started => PendingEvent::new(
                    subject,
                    &DiscountStarted {
                        discount,
                        method,
                        starts_at: at.unix_micros(),
                    },
                ),
                Boundary::Ended => PendingEvent::new(
                    subject,
                    &DiscountEnded {
                        discount,
                        method,
                        ends_at: at.unix_micros(),
                    },
                ),
            }
        })
        .collect();
    let count = u32::try_from(events.len()).unwrap_or(u32::MAX);
    let mut batch = ports.batch();
    batch.record_events(events);
    batch.advance_discount_schedule(Some(after), now);
    match ports.commit(batch).await {
        Ok(()) => Ok(Announced {
            events: count,
            next,
        }),
        // Another run announced them.
        Err(RepoError::StaleVersion) => Ok(Announced { events: 0, next }),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;
    use crate::Method;

    fn at(micros: i64) -> Timestamp {
        Timestamp::from_unix_micros(micros)
    }

    fn id(n: u128) -> DiscountId {
        DiscountId::from_uuid(Uuid::from_u128(n))
    }

    fn window(n: u128, starts: Option<i64>, ends: Option<i64>) -> Window {
        Window {
            discount: id(n),
            method: Method::Automatic,
            starts_at: starts.map(at),
            ends_at: ends.map(at),
        }
    }

    #[test]
    fn a_windows_boundaries_are_announced() {
        let windows = [window(1, Some(110), Some(140))];
        assert_eq!(
            boundaries(&windows, at(100), at(150)),
            vec![
                (id(1), Boundary::Started, at(110)),
                (id(1), Boundary::Ended, at(140)),
            ]
        );
    }

    #[test]
    fn the_next_boundary_is_the_earliest_ahead() {
        let windows = [window(1, Some(100), Some(300)), window(2, Some(200), None)];
        assert_eq!(next_boundary(&windows, at(150)), Some(at(200)));
        assert_eq!(next_boundary(&windows, at(300)), None);
    }
}
