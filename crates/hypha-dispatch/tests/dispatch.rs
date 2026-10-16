//! The dispatcher in memory (design note 0016, sections 5 and 6).

use std::cell::RefCell;

use hypha_apps::{App, AppKey, AppRepo, NewApp, Webhook};
use hypha_core::{
    BasisPoints, Clock, Currency, Delivery, Money, OutboxRepo, Price, PriceListId, ProductId,
    Quantity, Sealed, VariantId,
};
use hypha_dispatch::{
    DispatchSettings, Progress, WebhookSender, deliver_due, fan_out, prune_events,
};
use hypha_inventory::{InventoryItem, InventoryItemRepo, Location, ReservationRef};
use hypha_markets::{PriceList, PriceListRepo};
use hypha_products::{
    Handle, NewVariant, OptionValue, Product, ProductOption, ProductRepo, remove_product_variant,
};
use hypha_testkit::{FixedClock, InMemoryPorts};
use pollster::block_on;
use uuid::Uuid;

fn settings() -> DispatchSettings {
    DispatchSettings::default()
}

fn eur(amount: i64) -> Price {
    Price::new(Money::new(amount, Currency::Eur).unwrap()).unwrap()
}
fn product_id() -> ProductId {
    ProductId::from_uuid(Uuid::from_u128(1))
}
fn small() -> VariantId {
    VariantId::from_uuid(Uuid::from_u128(11))
}
fn large() -> VariantId {
    VariantId::from_uuid(Uuid::from_u128(12))
}
fn list(n: u128) -> PriceListId {
    PriceListId::from_uuid(Uuid::from_u128(100 + n))
}

/// A shirt in S and L, both with fixed prices in two price lists and an inventory item.
fn world() -> InMemoryPorts {
    let ports = InMemoryPorts::default();
    let size = ProductOption::new(
        hypha_core::OptionId::from_uuid(Uuid::from_u128(20)),
        "Size".to_string(),
        vec![
            OptionValue::new(
                hypha_core::OptionValueId::from_uuid(Uuid::from_u128(21)),
                "S".to_string(),
            ),
            OptionValue::new(
                hypha_core::OptionValueId::from_uuid(Uuid::from_u128(22)),
                "L".to_string(),
            ),
        ],
    );
    let variant = |id: VariantId, value: u128| NewVariant {
        id,
        sku: None,
        price: eur(1000),
        selections: vec![hypha_core::OptionValueId::from_uuid(Uuid::from_u128(value))],
    };
    let mut shirt = Product::new_configurable(
        product_id(),
        Handle::new("shirt").unwrap(),
        "Shirt".to_string(),
        vec![size],
        variant(small(), 21),
    )
    .unwrap();
    shirt.add_variant(variant(large(), 22)).unwrap();
    block_on(ports.products.insert(&mut shirt)).unwrap();
    for n in [1, 2] {
        let mut prices = PriceList::new(list(n), Currency::Eur, BasisPoints::new(0).unwrap());
        block_on(ports.price_lists.insert(&mut prices)).unwrap();
        block_on(
            ports
                .price_lists
                .set_fixed_prices(list(n), &[(small(), eur(900)), (large(), eur(1100))]),
        )
        .unwrap();
    }
    for (n, variant) in [(31, small()), (32, large())] {
        let id = hypha_core::InventoryItemId::from_uuid(Uuid::from_u128(n));
        let mut item = InventoryItem::new(id, variant, true, false);
        block_on(ports.inventory_items.insert(&mut item)).unwrap();
    }
    ports
}

fn remove(ports: &InMemoryPorts, variant: VariantId) {
    block_on(remove_product_variant(
        &hypha_core::Ctx::system(),
        &ports.products,
        product_id(),
        variant,
    ))
    .unwrap();
}

/// Records what it was asked to send, and answers `fails` when set.
#[derive(Default)]
struct Webhooks {
    sent: RefCell<Vec<(String, String)>>,
    fails: Option<String>,
}

impl WebhookSender for Webhooks {
    async fn send(&self, app: &App, delivery: &Delivery) -> Result<(), String> {
        self.sent
            .borrow_mut()
            .push((app.key().to_string(), delivery.event.event_type.clone()));
        self.fails.clone().map_or(Ok(()), Err)
    }
}

fn dispatch(ports: &InMemoryPorts, clock: &FixedClock) -> Progress {
    dispatch_with(ports, clock, &Webhooks::default())
}

fn dispatch_with(ports: &InMemoryPorts, clock: &FixedClock, webhooks: &Webhooks) -> Progress {
    block_on(fan_out(ports, clock, &settings())).unwrap();
    block_on(deliver_due(ports, clock, &settings(), webhooks)).unwrap()
}

fn cleanup_deliveries(ports: &InMemoryPorts) -> Vec<(String, &'static str, u32, Option<String>)> {
    ports
        .outbox
        .deliveries()
        .into_iter()
        .filter(|d| d.0 == "handler:variant_cleanup")
        .collect()
}

#[test]
fn a_removed_variant_loses_its_prices_and_stock_record() {
    let ports = world();
    let clock = FixedClock::default();
    remove(&ports, small());
    assert_eq!(dispatch(&ports, &clock).done, 1);

    for n in [1, 2] {
        let prices =
            block_on(ports.price_lists.fixed_prices(list(n), &[small(), large()])).unwrap();
        assert!(!prices.contains_key(&small()), "list {n}");
        assert!(
            prices.contains_key(&large()),
            "list {n} keeps the other variant"
        );
    }
    assert_eq!(
        block_on(ports.inventory_items.get_by_variant(small())),
        Ok(None)
    );
    assert!(
        block_on(ports.inventory_items.get_by_variant(large()))
            .unwrap()
            .is_some()
    );
    assert_eq!(
        cleanup_deliveries(&ports),
        [("handler:variant_cleanup".to_string(), "succeeded", 1, None)]
    );
    // The cleanup's own writes are events too, for search.
    let types = ports.outbox.types();
    assert_eq!(
        types
            .iter()
            .filter(|t| *t == "price_list.prices_removed")
            .count(),
        2
    );
    assert!(types.contains(&"inventory_item.deleted".to_string()));
}

#[test]
fn an_item_with_reservations_is_retired_until_its_orders_are_done() {
    let ports = world();
    let clock = FixedClock::default();
    let mut item = block_on(ports.inventory_items.get_by_variant(small()))
        .unwrap()
        .unwrap();
    let location = Location::new(
        hypha_core::LocationId::from_uuid(Uuid::from_u128(40)),
        "Warehouse".to_string(),
        vec![],
        0,
    )
    .unwrap();
    item.set_on_hand(location.id(), 5, None).unwrap();
    let order_line = ReservationRef::new("line-1").unwrap();
    item.reserve(&order_line, Quantity::new(1).unwrap(), &[location])
        .unwrap();
    block_on(ports.inventory_items.update(&mut item)).unwrap();

    remove(&ports, small());
    dispatch(&ports, &clock);
    let kept = block_on(ports.inventory_items.get_by_variant(small()))
        .unwrap()
        .unwrap();
    // Kept with its reservation, and marked to go with it (design note 0018).
    assert!(kept.retired());
    assert_eq!(kept.reservations().len(), 1);
    // Not a failure: there's nothing more to do now.
    assert_eq!(cleanup_deliveries(&ports)[0].1, "succeeded");
}

#[test]
fn running_a_cleanup_again_is_harmless() {
    let ports = world();
    let clock = FixedClock::default();
    remove(&ports, small());
    block_on(fan_out(&ports, &clock, &settings())).unwrap();
    // A runner leases it and dies; the lease runs out and another runs it, then a
    // third time, as at-least-once allows.
    let leased = block_on(hypha_dispatch::lease_due(&ports, &clock, &settings())).unwrap();
    assert_eq!(leased.len(), 1);
    block_on(hypha_dispatch::run(
        &ports,
        &clock,
        &settings(),
        &Webhooks::default(),
        &leased[0],
    ));
    clock.set(FixedClock::DEFAULT.plus_secs(120));
    assert_eq!(
        block_on(deliver_due(
            &ports,
            &clock,
            &settings(),
            &Webhooks::default()
        ))
        .unwrap()
        .done,
        1
    );
    assert_eq!(cleanup_deliveries(&ports)[0].1, "succeeded");
    assert_eq!(
        block_on(ports.inventory_items.get_by_variant(small())),
        Ok(None)
    );
}

#[test]
fn a_failing_delivery_is_retried_on_the_schedule_then_fails() {
    let ports = world();
    let clock = FixedClock::default();
    // A destination this build has no handler for: every attempt fails.
    block_on(
        ports
            .outbox
            .fan_out(10, clock.now(), &|_| vec!["handler:gone".to_string()]),
    )
    .unwrap();
    assert!(
        block_on(deliver_due(
            &ports,
            &clock,
            &settings(),
            &Webhooks::default()
        ))
        .unwrap()
        .done
            > 0
    );
    let failing: Vec<_> = ports
        .outbox
        .deliveries()
        .into_iter()
        .filter(|d| d.0 == "handler:gone")
        .collect();
    assert!(failing.iter().all(|d| d.1 == "pending" && d.2 == 1));
    // Not due again before its minute is up.
    clock.set(FixedClock::DEFAULT.plus_secs(59));
    assert_eq!(
        block_on(deliver_due(
            &ports,
            &clock,
            &settings(),
            &Webhooks::default()
        ))
        .unwrap()
        .done,
        0
    );
    // Keep running the dispatcher until nothing is due any more.
    let mut at = 59;
    for _ in 0..200 {
        at += 3600;
        clock.set(FixedClock::DEFAULT.plus_secs(at));
        block_on(deliver_due(
            &ports,
            &clock,
            &settings(),
            &Webhooks::default(),
        ))
        .unwrap();
    }
    let failing: Vec<_> = ports
        .outbox
        .deliveries()
        .into_iter()
        .filter(|d| d.0 == "handler:gone")
        .collect();
    assert!(
        failing
            .iter()
            .all(|d| d.1 == "failed" && d.2 == settings().retry.max_attempts()),
        "{failing:?}"
    );
    assert!(failing[0].3.as_deref().unwrap().contains("no handler"));
}

#[test]
fn old_events_are_pruned_after_thirty_days() {
    let ports = world();
    let clock = FixedClock::default();
    remove(&ports, small());
    dispatch(&ports, &clock);
    let written = ports.outbox.events().len();
    assert!(written > 0);
    // Events occur at the ports' clock (the fixed default): not old yet.
    assert_eq!(
        block_on(prune_events(&ports, &clock, &settings()))
            .unwrap()
            .done,
        0
    );
    clock.set(FixedClock::DEFAULT.plus_secs(31 * 24 * 3600));
    // The cleanup's own events weren't fanned out yet: they stay until they are.
    let pruned = block_on(prune_events(&ports, &clock, &settings()))
        .unwrap()
        .done as usize;
    assert!(pruned > 0 && pruned < written, "{pruned} of {written}");
    block_on(fan_out(&ports, &clock, &settings())).unwrap();
    block_on(prune_events(&ports, &clock, &settings())).unwrap();
    assert!(ports.outbox.events().is_empty());
}

// ---------- webhooks ----------

fn app(key: &str, subscriptions: &[&str], webhook: bool) -> App {
    App::new(NewApp {
        key: AppKey::new(key).unwrap(),
        name: key.to_string(),
        secret: Sealed::from_bytes(vec![1, 2, 3]),
        webhook: webhook.then(|| Webhook::new("https://erp.example/hooks", None).unwrap()),
        subscriptions: subscriptions.iter().map(|s| s.to_string()).collect(),
    })
    .unwrap()
}

fn app_deliveries(ports: &InMemoryPorts) -> Vec<(String, &'static str, u32, Option<String>)> {
    ports
        .outbox
        .deliveries()
        .into_iter()
        .filter(|d| d.0.starts_with("app:"))
        .collect()
}

#[test]
fn subscribed_apps_get_webhooks() {
    let ports = world();
    let clock = FixedClock::default();
    for mut app in [
        app("app_erp", &["product.variant_removed"], true),
        app("app_crm", &["order.*"], true),
        app("app_rates", &["product.variant_removed"], false),
    ] {
        block_on(ports.apps.insert(&mut app)).unwrap();
    }
    let mut off = app("app_off", &["product.*"], true);
    off.set_enabled(false);
    block_on(ports.apps.insert(&mut off)).unwrap();
    remove(&ports, small());

    let webhooks = Webhooks::default();
    dispatch_with(&ports, &clock, &webhooks);
    // Only the enabled app with a webhook subscribed to the type.
    assert_eq!(
        webhooks.sent.borrow().as_slice(),
        [("app_erp".to_string(), "product.variant_removed".to_string())]
    );
    assert_eq!(
        app_deliveries(&ports),
        [("app:app_erp".to_string(), "succeeded", 1, None)]
    );
}

#[test]
fn a_failing_webhook_is_retried() {
    let ports = world();
    let clock = FixedClock::default();
    block_on(
        ports
            .apps
            .insert(&mut app("app_erp", &["product.variant_removed"], true)),
    )
    .unwrap();
    remove(&ports, small());
    let webhooks = Webhooks {
        fails: Some("answered HTTP 503".to_string()),
        ..Webhooks::default()
    };
    dispatch_with(&ports, &clock, &webhooks);
    assert_eq!(
        app_deliveries(&ports),
        [(
            "app:app_erp".to_string(),
            "pending",
            1,
            Some("answered HTTP 503".to_string())
        )]
    );
}

#[test]
fn a_webhook_with_nowhere_to_go_fails_at_once_and_a_paused_app_waits() {
    let ports = world();
    let clock = FixedClock::default();
    for mut app in [
        app("app_gone", &["product.variant_removed"], true),
        app("app_off", &["product.variant_removed"], true),
        app("app_bare", &["product.variant_removed"], true),
    ] {
        block_on(ports.apps.insert(&mut app)).unwrap();
    }
    remove(&ports, small());
    block_on(fan_out(&ports, &clock, &settings())).unwrap();
    // Between fanning out and sending, the apps change.
    block_on(ports.apps.delete(&AppKey::new("app_gone").unwrap())).unwrap();
    let mut off = block_on(ports.apps.get(&AppKey::new("app_off").unwrap()))
        .unwrap()
        .unwrap();
    off.set_enabled(false);
    block_on(ports.apps.update(&mut off)).unwrap();
    let mut bare = block_on(ports.apps.get(&AppKey::new("app_bare").unwrap()))
        .unwrap()
        .unwrap();
    bare.set_webhook(None);
    block_on(ports.apps.update(&mut bare)).unwrap();

    let webhooks = Webhooks::default();
    block_on(deliver_due(&ports, &clock, &settings(), &webhooks)).unwrap();
    assert!(webhooks.sent.borrow().is_empty());
    let mut failed = app_deliveries(&ports);
    failed.sort();
    assert_eq!(
        failed,
        [
            (
                "app:app_bare".to_string(),
                "failed",
                1,
                Some("app app_bare has no webhook".to_string())
            ),
            // app_gone's went with it.
            // A paused app keeps its backlog, on the retry schedule.
            (
                "app:app_off".to_string(),
                "pending",
                1,
                Some("app app_off is disabled".to_string())
            ),
        ]
    );
}

#[test]
fn a_delivery_starts_only_with_time_left_on_its_lease() {
    let ports = world();
    let clock = FixedClock::default();
    remove(&ports, small());
    block_on(fan_out(&ports, &clock, &settings())).unwrap();
    let leased = block_on(hypha_dispatch::lease_due(&ports, &clock, &settings())).unwrap();
    // Leased for 120 s; the longest run is 45 s (a 30 s webhook and room).
    clock.set(FixedClock::DEFAULT.plus_secs(75));
    assert!(hypha_dispatch::has_time(&leased[0], &clock));
    clock.set(FixedClock::DEFAULT.plus_secs(76));
    assert!(!hypha_dispatch::has_time(&leased[0], &clock));
}
