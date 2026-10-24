//! Announcing price list windows: prices follow a list's window by themselves (pricing reads
//! the time), but what's computed ahead (search documents, cached pages, apps) learns that
//! a window opened or closed only from `price_list.started` / `price_list.ended`, recorded
//! here by the schedule job.

use hypha_core::{
    CatalogId, Clock, HasBatch, PendingEvent, PriceListId, RepoError, StageEvents, Stamped,
    Subject, Timestamp,
};

use crate::{
    CatalogRepo, HasCatalogs, HasPriceLists, PriceList, PriceListRepo, PriceListStatus,
    StagePriceListSchedule,
    events::{PRICE_LIST, PriceListEnded, PriceListStarted},
};

/// Which end of a window passed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Boundary {
    Started,
    Ended,
}

/// The window boundaries of `lists` that passed in `(after, until]`, oldest first (then by
/// list, a start before an end). Only an active list's count: a draft prices nothing either
/// side. A boundary at or before the list's last write isn't one to announce: that write
/// recorded its own event, after which the list already priced as it does now.
pub fn boundaries(
    lists: &[PriceList],
    after: Timestamp,
    until: Timestamp,
) -> Vec<(PriceListId, Boundary, Timestamp)> {
    let mut found = Vec::new();
    for list in lists {
        if list.status() != PriceListStatus::Active {
            continue;
        }
        let written = list.stamps().map(|s| s.updated_at);
        let window = list.window();
        for (boundary, at) in [
            (Boundary::Started, window.starts_at()),
            (Boundary::Ended, window.ends_at()),
        ] {
            let Some(at) = at else { continue };
            if at > after && at <= until && written.is_none_or(|w| w < at) {
                found.push((list.id(), boundary, at));
            }
        }
    }
    found.sort_by_key(|(list, boundary, at)| (*at, *list, *boundary));
    found
}

/// The first window boundary of an active list after `after`: when the job next has
/// something to announce, unless a list changes before then.
pub fn next_boundary(lists: &[PriceList], after: Timestamp) -> Option<Timestamp> {
    lists
        .iter()
        .filter(|l| l.status() == PriceListStatus::Active)
        .flat_map(|l| [l.window().starts_at(), l.window().ends_at()])
        .flatten()
        .filter(|at| *at > after)
        .min()
}

/// What a run of the schedule job did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Announced {
    /// Events recorded.
    pub events: u32,
    /// The next boundary after now, for a host that runs jobs only when they're due.
    pub next: Option<Timestamp>,
}

/// Records `price_list.started` / `price_list.ended` for every boundary that passed since
/// the last run that announced one, up to now, with the progress in the same commit. Runs
/// may race: the one that commits first announces, the other finds its progress moved
/// (`StaleVersion`) and announces nothing. The first run ever only notes where it starts:
/// boundaries before the job ran were before anyone was told to expect them.
pub async fn announce_windows<S>(ports: &S, clock: &impl Clock) -> Result<Announced, RepoError>
where
    S: HasPriceLists + HasCatalogs + HasBatch,
    S::Batch: StageEvents + StagePriceListSchedule,
{
    let now = clock.now();
    let ran_until = ports.price_lists().schedule_ran_until().await?;
    let lists = ports.price_lists().list_all().await?;
    let next = next_boundary(&lists, now);
    let Some(after) = ran_until else {
        let mut batch = ports.batch();
        batch.advance_price_list_schedule(None, now);
        return match ports.commit(batch).await {
            Ok(()) | Err(RepoError::StaleVersion) => Ok(Announced { events: 0, next }),
            Err(e) => Err(e),
        };
    };
    let passed = boundaries(&lists, after, now);
    if passed.is_empty() {
        // Nothing to write: the next run looks over this stretch again and finds the same.
        return Ok(Announced { events: 0, next });
    }
    let catalogs = ports.catalogs().list_all().await?;
    let holding = |list: PriceListId| -> Vec<CatalogId> {
        catalogs
            .iter()
            .filter(|c| c.price_lists().contains(&list))
            .map(|c| c.id())
            .collect()
    };
    let events: Vec<PendingEvent> = passed
        .iter()
        .map(|&(list, boundary, at)| {
            let subject = Subject::new(PRICE_LIST, list.as_uuid());
            let catalogs = holding(list);
            match boundary {
                Boundary::Started => PendingEvent::new(
                    subject,
                    &PriceListStarted {
                        price_list: list,
                        catalogs,
                        starts_at: at.unix_micros(),
                    },
                ),
                Boundary::Ended => PendingEvent::new(
                    subject,
                    &PriceListEnded {
                        price_list: list,
                        catalogs,
                        ends_at: at.unix_micros(),
                    },
                ),
            }
        })
        .collect();
    let count = u32::try_from(events.len()).unwrap_or(u32::MAX);
    let mut batch = ports.batch();
    batch.record_events(events);
    batch.advance_price_list_schedule(Some(after), now);
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
    use hypha_core::{Currency, Stamps};
    use uuid::Uuid;

    use super::*;
    use crate::PriceListDraft;

    fn at(micros: i64) -> Timestamp {
        Timestamp::from_unix_micros(micros)
    }

    fn list(n: u128, starts: Option<i64>, ends: Option<i64>, written: i64) -> PriceList {
        let mut list = PriceList::new(
            PriceListId::from_uuid(Uuid::from_u128(n)),
            PriceListDraft {
                starts_at: starts.map(at),
                ends_at: ends.map(at),
                ..PriceListDraft::new(&format!("List {n}"), Currency::EUR)
            },
        )
        .unwrap();
        list.set_stamps(Stamps::new(at(written)));
        list
    }

    fn id(n: u128) -> PriceListId {
        PriceListId::from_uuid(Uuid::from_u128(n))
    }

    #[test]
    fn a_start_inside_the_stretch_is_announced() {
        let lists = [list(1, Some(100), None, 0)];
        assert_eq!(
            boundaries(&lists, at(50), at(150)),
            vec![(id(1), Boundary::Started, at(100))]
        );
    }

    #[test]
    fn a_boundary_at_the_stretchs_start_was_the_last_runs() {
        let lists = [list(1, Some(100), None, 0)];
        assert_eq!(boundaries(&lists, at(100), at(150)), vec![]);
    }

    #[test]
    fn a_boundary_at_the_stretchs_end_counts() {
        let lists = [list(1, None, Some(150), 0)];
        assert_eq!(
            boundaries(&lists, at(100), at(150)),
            vec![(id(1), Boundary::Ended, at(150))]
        );
    }

    #[test]
    fn a_boundary_still_ahead_waits() {
        let lists = [list(1, Some(151), None, 0)];
        assert_eq!(boundaries(&lists, at(100), at(150)), vec![]);
    }

    #[test]
    fn a_draft_announces_nothing() {
        let mut draft = list(1, Some(120), None, 0);
        draft.set_status(PriceListStatus::Draft);
        assert_eq!(boundaries(&[draft], at(100), at(150)), vec![]);
    }

    #[test]
    fn a_list_written_after_its_boundary_announced_it_itself() {
        let lists = [list(1, Some(120), None, 130)];
        assert_eq!(boundaries(&lists, at(100), at(150)), vec![]);
    }

    #[test]
    fn a_whole_window_passing_announces_both_ends_in_order() {
        let lists = [
            list(2, Some(110), Some(140), 0),
            list(1, Some(120), None, 0),
        ];
        assert_eq!(
            boundaries(&lists, at(100), at(150)),
            vec![
                (id(2), Boundary::Started, at(110)),
                (id(1), Boundary::Started, at(120)),
                (id(2), Boundary::Ended, at(140)),
            ]
        );
    }

    #[test]
    fn the_next_boundary_is_the_earliest_ahead() {
        let mut draft = list(3, Some(160), None, 0);
        draft.set_status(PriceListStatus::Draft);
        let lists = [
            list(1, Some(100), Some(300), 0),
            list(2, Some(200), None, 0),
            draft,
        ];
        assert_eq!(next_boundary(&lists, at(150)), Some(at(200)));
        assert_eq!(next_boundary(&lists, at(300)), None);
    }
}
