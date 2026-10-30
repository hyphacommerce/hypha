//! The schedule job announcing discount windows.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a failing test panics"
)]
use hypha_core::{BasisPoints, DiscountId, HasBatch, RepoError, Timestamp, schedule::Announced};
use hypha_discounts::{
    Discount, DiscountDraft, DiscountRepo, HasDiscounts, Kind, Method, Reward, StageDiscounts,
    Target, Value, announce_windows,
};
use hypha_testkit::{FixedClock, InMemoryPorts};
use pollster::block_on;
use serde_json::json;
use uuid::Uuid;

const MINUTE: i64 = 60_000_000;

fn later(minutes: i64) -> Timestamp {
    FixedClock::DEFAULT.plus(std::time::Duration::from_micros((minutes * MINUTE) as u64))
}

fn did(n: u128) -> DiscountId {
    DiscountId::from_uuid(Uuid::from_u128(n))
}

/// A store with an automatic 10% off from minute 10 to minute 20, made at the start.
fn store() -> InMemoryPorts {
    let ports = InMemoryPorts::default();
    let kind = Kind::Standard {
        rewards: vec![Reward {
            target: Target::Subtotal,
            value: Value::Percent(BasisPoints::new(1000).unwrap()),
        }],
    };
    let mut sale = Discount::new(
        did(1),
        DiscountDraft {
            starts_at: Some(later(10)),
            ends_at: Some(later(20)),
            ..DiscountDraft::new("Weekend", Method::Automatic, kind)
        },
    )
    .unwrap();
    block_on(ports.discounts().insert(&mut sale)).unwrap();
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
        .filter(|e| e.event_type == "discount.started" || e.event_type == "discount.ended")
        .map(|e| (e.event_type.to_string(), e.data))
        .collect()
}

#[test]
fn the_first_run_only_notes_where_it_starts() {
    let ports = store();
    assert_eq!(
        run(&ports, later(15)),
        Announced {
            events: 0,
            next: Some(later(20))
        }
    );
    assert_eq!(announced(&ports), vec![]);
    assert_eq!(
        block_on(ports.discounts().schedule_ran_until()),
        Ok(Some(later(15)))
    );
}

#[test]
fn each_boundary_is_announced_once() {
    let ports = store();
    run(&ports, later(1));
    assert_eq!(
        run(&ports, later(11)),
        Announced {
            events: 1,
            next: Some(later(20))
        }
    );
    assert_eq!(run(&ports, later(12)).events, 0);
    assert_eq!(
        run(&ports, later(21)),
        Announced {
            events: 1,
            next: None
        }
    );
    assert_eq!(run(&ports, later(30)).events, 0);
    assert_eq!(
        announced(&ports),
        vec![
            (
                "discount.started".to_string(),
                json!({
                    "discount": did(1).to_string(),
                    "method": "automatic",
                    "starts_at": later(10).unix_micros(),
                })
            ),
            (
                "discount.ended".to_string(),
                json!({
                    "discount": did(1).to_string(),
                    "method": "automatic",
                    "ends_at": later(20).unix_micros(),
                })
            ),
        ]
    );
}

#[test]
fn a_rename_after_the_boundary_doesnt_suppress_it() {
    let ports = store();
    run(&ports, later(1));
    // Renamed at minute 12, after it started but before the job looked again.
    ports.outbox.clock().set(later(12));
    let mut discount = block_on(ports.discounts().get(did(1))).unwrap().unwrap();
    let mut draft = discount.draft();
    draft.title = "Long weekend".to_string();
    discount.update(draft).unwrap();
    block_on(ports.discounts().update(&mut discount)).unwrap();
    assert_eq!(run(&ports, later(13)).events, 1);
    let types: Vec<String> = announced(&ports).into_iter().map(|(t, _)| t).collect();
    assert_eq!(types, ["discount.started"]);
}

#[test]
fn a_missed_stretch_catches_up_in_order() {
    let ports = store();
    run(&ports, later(1));
    assert_eq!(
        run(&ports, later(30)),
        Announced {
            events: 2,
            next: None
        }
    );
    let types: Vec<String> = announced(&ports).into_iter().map(|(t, _)| t).collect();
    assert_eq!(types, ["discount.started", "discount.ended"]);
}

#[test]
fn a_run_that_lost_the_race_announces_nothing() {
    let ports = store();
    run(&ports, later(1));
    let mut batch = ports.batch();
    batch.advance_discount_schedule(Some(later(1)), later(5));
    block_on(ports.commit(batch)).unwrap();
    let mut stale = ports.batch();
    stale.advance_discount_schedule(Some(later(1)), later(11));
    assert_eq!(block_on(ports.commit(stale)), Err(RepoError::StaleVersion));
    assert_eq!(announced(&ports), vec![]);
    assert_eq!(run(&ports, later(11)).events, 1);
}
