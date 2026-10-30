//! The schedule job announcing price list windows.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a failing test panics"
)]
use hypha_core::{CatalogId, Currency, HasBatch, MarketId, PriceListId, RepoError, Timestamp};
use hypha_markets::{
    Announced, Catalog, CatalogContext, CatalogRepo, HasCatalogs, HasPriceLists, PriceList,
    PriceListDraft, PriceListRepo, StagePriceListSchedule, announce_windows,
};
use hypha_testkit::{FixedClock, InMemoryPorts};
use pollster::block_on;
use serde_json::json;
use uuid::Uuid;

const MINUTE: i64 = 60_000_000;

fn start() -> Timestamp {
    FixedClock::DEFAULT
}

fn later(minutes: i64) -> Timestamp {
    start().plus(std::time::Duration::from_micros((minutes * MINUTE) as u64))
}

fn plid(n: u128) -> PriceListId {
    PriceListId::from_uuid(Uuid::from_u128(n))
}

/// A store with a sale from minute 10 to minute 20 on market 1's catalog, made at the
/// start.
fn store() -> InMemoryPorts {
    let ports = InMemoryPorts::default();
    let mut sale = PriceList::new(
        plid(1),
        PriceListDraft {
            starts_at: Some(later(10)),
            ends_at: Some(later(20)),
            ..PriceListDraft::new("Sale", Currency::EUR)
        },
    )
    .unwrap();
    block_on(ports.price_lists().insert(&mut sale)).unwrap();
    let mut catalog = Catalog::new(
        CatalogId::from_uuid(Uuid::from_u128(5)),
        CatalogContext::Market(MarketId::from_uuid(Uuid::from_u128(9))),
        [plid(1)],
    );
    block_on(ports.catalogs().insert(&mut catalog)).unwrap();
    ports
}

fn run(ports: &InMemoryPorts, at: Timestamp) -> Announced {
    ports.outbox.clock().set(at);
    block_on(announce_windows(ports, ports.outbox.clock())).unwrap()
}

fn announced(ports: &InMemoryPorts) -> Vec<(String, serde_json::Value)> {
    ports
        .outbox
        .events()
        .into_iter()
        .filter(|e| e.event_type.starts_with("price_list.") && e.event_type != "price_list.created")
        .map(|e| (e.event_type.to_string(), e.data))
        .collect()
}

#[test]
fn the_first_run_only_notes_where_it_starts() {
    let ports = store();
    let first = run(&ports, later(15));
    assert_eq!(
        first,
        Announced {
            events: 0,
            next: Some(later(20))
        }
    );
    assert_eq!(announced(&ports), vec![]);
    assert_eq!(
        block_on(ports.price_lists().schedule_ran_until()),
        Ok(Some(later(15)))
    );
}

#[test]
fn a_window_opening_is_announced_once_with_its_catalogs() {
    let ports = store();
    run(&ports, later(1));
    let ran = run(&ports, later(11));
    assert_eq!(
        ran,
        Announced {
            events: 1,
            next: Some(later(20))
        }
    );
    run(&ports, later(12));
    assert_eq!(
        announced(&ports),
        vec![(
            "price_list.started".to_string(),
            json!({
                "price_list": plid(1).to_string(),
                "catalogs": [CatalogId::from_uuid(Uuid::from_u128(5)).to_string()],
                "starts_at": later(10).unix_micros(),
            })
        )]
    );
}

#[test]
fn a_missed_stretch_catches_up_in_order() {
    let ports = store();
    run(&ports, later(1));
    let ran = run(&ports, later(30));
    assert_eq!(
        ran,
        Announced {
            events: 2,
            next: None
        }
    );
    let types: Vec<String> = announced(&ports).into_iter().map(|(t, _)| t).collect();
    assert_eq!(types, ["price_list.started", "price_list.ended"]);
}

#[test]
fn a_run_that_lost_the_race_announces_nothing() {
    let ports = store();
    run(&ports, later(1));
    // Another runner moved it first.
    let mut batch = ports.batch();
    batch.advance_price_list_schedule(Some(later(1)), later(5));
    block_on(ports.commit(batch)).unwrap();
    // A run that read the old progress commits after it: stale, nothing written.
    let mut stale = ports.batch();
    stale.advance_price_list_schedule(Some(later(1)), later(11));
    assert_eq!(block_on(ports.commit(stale)), Err(RepoError::StaleVersion));
    // The next real run starts where the other left off and announces the start once.
    assert_eq!(run(&ports, later(11)).events, 1);
}
