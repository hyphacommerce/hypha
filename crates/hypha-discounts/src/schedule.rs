//! Announcing discount windows: carts follow a discount's window by themselves (they read
//! the time), but what's computed or cached ahead (storefront pages and their promotion
//! badges, apps) learns that a window opened or closed only from `discount.started` /
//! `discount.ended`, recorded here by the schedule job.

use hypha_core::{
    Clock, DiscountId, HasBatch, PendingEvent, RepoError, StageEvents, Subject, Timestamp,
    schedule::{self, Announced, Boundary},
};

use crate::{
    Discount, DiscountRepo, HasDiscounts, StageDiscounts, Status,
    events::{DISCOUNT, DiscountEnded, DiscountStarted},
};

/// The window boundaries of `discounts` that passed in `(after, until]`, oldest first
/// (then by discount, a start before an end). Only an active discount's count: a draft
/// applies nothing either side. Each is announced even when the discount was written after
/// it: that write's `discount.updated` may have said nothing about its window (a rename).
pub fn boundaries(
    discounts: &[Discount],
    after: Timestamp,
    until: Timestamp,
) -> Vec<(DiscountId, Boundary, Timestamp)> {
    schedule::passed(
        active(discounts).map(|d| (d.id(), d.starts_at(), d.ends_at())),
        after,
        until,
    )
}

/// The first window boundary of an active discount after `after`.
pub fn next_boundary(discounts: &[Discount], after: Timestamp) -> Option<Timestamp> {
    schedule::next(
        active(discounts).map(|d| (d.starts_at(), d.ends_at())),
        after,
    )
}

fn active(discounts: &[Discount]) -> impl Iterator<Item = &Discount> {
    discounts.iter().filter(|d| d.status() == Status::Active)
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
            .find(|d| d.id() == id)
            .map_or("", |d| d.method().as_str())
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
    use hypha_core::BasisPoints;
    use uuid::Uuid;

    use super::*;
    use crate::{DiscountDraft, Kind, Method, Reward, Target, Value};

    fn at(micros: i64) -> Timestamp {
        Timestamp::from_unix_micros(micros)
    }

    fn id(n: u128) -> DiscountId {
        DiscountId::from_uuid(Uuid::from_u128(n))
    }

    fn discount(n: u128, status: Status, starts: Option<i64>, ends: Option<i64>) -> Discount {
        let kind = Kind::Standard {
            rewards: vec![Reward {
                target: Target::Subtotal,
                value: Value::Percent(BasisPoints::new(1000).unwrap()),
            }],
        };
        Discount::new(
            id(n),
            DiscountDraft {
                status,
                starts_at: starts.map(at),
                ends_at: ends.map(at),
                ..DiscountDraft::new(&format!("Discount {n}"), Method::Automatic, kind)
            },
        )
        .unwrap()
    }

    #[test]
    fn an_active_discounts_boundaries_are_announced() {
        let discounts = [discount(1, Status::Active, Some(110), Some(140))];
        assert_eq!(
            boundaries(&discounts, at(100), at(150)),
            vec![
                (id(1), Boundary::Started, at(110)),
                (id(1), Boundary::Ended, at(140)),
            ]
        );
    }

    #[test]
    fn a_draft_announces_nothing_and_isnt_waited_for() {
        let discounts = [discount(1, Status::Draft, Some(120), Some(200))];
        assert_eq!(boundaries(&discounts, at(100), at(150)), vec![]);
        assert_eq!(next_boundary(&discounts, at(100)), None);
    }

    #[test]
    fn the_next_boundary_is_the_earliest_ahead() {
        let discounts = [
            discount(1, Status::Active, Some(100), Some(300)),
            discount(2, Status::Active, Some(200), None),
        ];
        assert_eq!(next_boundary(&discounts, at(150)), Some(at(200)));
        assert_eq!(next_boundary(&discounts, at(300)), None);
    }
}
