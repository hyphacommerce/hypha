use std::ops::Deref;

use hypha_core::{
    Actor, AuthError, CartId, CountryCode, Ctx, Currency, CustomerId, Email, FulfillmentId,
    IdempotencyError, IdempotencyRequest, InventoryItemId, LocationId, MarketId, Money, OrderId,
    OrderLineId, Price, ProductId, Quantity, QuantityError, Scope, UserId, VariantId, Versioned,
};
use hypha_fulfillment::{FulfillmentError, FulfillmentRepo, FulfillmentState, TrackingInput};
use hypha_inventory::{InventoryItem, InventoryItemRepo, Location, LocationRepo, ReservationRef};
use hypha_order_actions::{
    CancelOrder, CommandError, Fulfil, FulfilLine, Fulfilled, MarkPaid, OrderPayments, OrderView,
    Unfulfilled, cancel_fulfillment, cancel_order, fulfill, mark_paid, order_view,
};
use hypha_orders::{
    CancelReason, FinancialStatus, FulfillmentStatus, NewOrder, Order, OrderError, OrderLine,
    OrderNumber, OrderRepo,
};
use hypha_payments::{
    NewPayment, Payment, PaymentError, PaymentRepo, PaymentSnapshot, PaymentStatus, ProviderKey,
};
use hypha_testkit::{FixedClock, InMemoryPorts, SequentialIds};
use pollster::block_on;
use uuid::Uuid;

fn order_id() -> OrderId {
    OrderId::from_uuid(Uuid::from_u128(1000))
}

/// 3 mugs, tracked, reserved at the warehouse.
fn mugs() -> OrderLineId {
    OrderLineId::from_uuid(Uuid::from_u128(1001))
}

/// 1 e-book, which has no inventory item (untracked).
fn ebook() -> OrderLineId {
    OrderLineId::from_uuid(Uuid::from_u128(1002))
}

fn mug_variant() -> VariantId {
    VariantId::from_uuid(Uuid::from_u128(2001))
}

fn warehouse() -> LocationId {
    LocationId::from_uuid(Uuid::from_u128(3001))
}

fn shop() -> LocationId {
    LocationId::from_uuid(Uuid::from_u128(3002))
}

fn mug_item() -> InventoryItemId {
    InventoryItemId::from_uuid(Uuid::from_u128(4001))
}

fn eur(amount: i64) -> Money {
    Money::new(amount, Currency::EUR).unwrap()
}

fn line(id: OrderLineId, variant: u128, quantity: u32) -> OrderLine {
    OrderLine {
        id,
        product: ProductId::from_uuid(Uuid::from_u128(variant + 100)),
        variant: VariantId::from_uuid(Uuid::from_u128(variant)),
        quantity: Quantity::new(quantity).unwrap(),
        product_title: "Thing".to_string(),
        variant_title: None,
        sku: None,
        unit_price: Price::new(eur(500)).unwrap(),
        total: eur(500 * i64::from(quantity)),
        tax: None,
        fulfilled_quantity: 0,
    }
}

/// A store with two locations (warehouse: 10 mugs, shop: 5) and order 1000: 3 mugs,
/// reserved at the warehouse, and an untracked e-book.
struct World {
    ports: InMemoryPorts,
    ids: SequentialIds,
    clock: FixedClock,
}

impl Deref for World {
    type Target = InMemoryPorts;
    fn deref(&self) -> &InMemoryPorts {
        &self.ports
    }
}

impl World {
    fn new() -> Self {
        let world = World {
            ports: InMemoryPorts::default(),
            ids: SequentialIds::default(),
            clock: FixedClock::default(),
        };
        let market = MarketId::from_uuid(Uuid::from_u128(5001));
        for (id, priority) in [(warehouse(), 0), (shop(), 1)] {
            let mut location =
                Location::new(id, "Place".to_string(), vec![market], priority).unwrap();
            block_on(world.locations.insert(&mut location)).unwrap();
        }
        let mut order = Order::place(NewOrder {
            id: order_id(),
            number: OrderNumber::new(1001).unwrap(),
            placed_at: FixedClock::DEFAULT,
            cart: CartId::from_uuid(Uuid::from_u128(6001)),
            channel: hypha_core::ChannelId::from_uuid(Uuid::from_u128(500)),
            market,
            currency: Currency::EUR,
            country: CountryCode::new("HR").unwrap(),
            taxes_included: true,
            customer: CustomerId::from_uuid(Uuid::from_u128(7001)),
            email: Email::new("ana@example.hr").unwrap(),
            lines: vec![line(mugs(), 2001, 3), line(ebook(), 2002, 1)],
            shipping_address: None,
            shipping_lines: vec![],
            subtotal: eur(2000),
            shipping_total: eur(0),
            tax_total: eur(0),
            total: eur(2000),
        })
        .unwrap();
        block_on(world.orders.insert(&mut order)).unwrap();

        let mut item = InventoryItem::new(mug_item(), mug_variant(), true, false);
        item.set_on_hand(warehouse(), 10, None).unwrap();
        item.set_on_hand(shop(), 5, None).unwrap();
        let locations = block_on(world.locations.serving(market)).unwrap();
        item.reserve(&reference(mugs()), Quantity::new(3).unwrap(), &locations)
            .unwrap();
        block_on(world.inventory_items.insert(&mut item)).unwrap();
        world
    }

    fn fulfil(
        &self,
        location: LocationId,
        lines: &[(OrderLineId, u32)],
    ) -> Result<Fulfilled, CommandError> {
        self.fulfil_as(&Ctx::system(), location, lines, None)
            .map(|done| done.value)
    }

    fn fulfil_as(
        &self,
        ctx: &Ctx,
        location: LocationId,
        lines: &[(OrderLineId, u32)],
        idempotency: Option<IdempotencyRequest>,
    ) -> Result<hypha_core::Idempotent<Fulfilled>, CommandError> {
        let input = Fulfil {
            order: order_id(),
            location,
            lines: lines
                .iter()
                .map(|&(line, quantity)| FulfilLine { line, quantity })
                .collect(),
            tracking: vec![TrackingInput {
                carrier: Some("DHL".to_string()),
                number: "JD0001".to_string(),
                url: Some("https://dhl.example/JD0001".to_string()),
            }],
            notify_customer: false,
            idempotency,
        };
        block_on(fulfill(ctx, &self.ports, &self.ids, &self.clock, input))
    }

    fn cancel(&self, reason: CancelReason) -> Result<Order, CommandError> {
        self.cancel_and_changed(reason).map(|(order, _)| order)
    }

    fn cancel_and_changed(&self, reason: CancelReason) -> Result<(Order, bool), CommandError> {
        let input = CancelOrder {
            order: order_id(),
            reason,
            notify_customer: false,
        };
        block_on(cancel_order(
            &Ctx::system(),
            &self.ports,
            &self.clock,
            input,
        ))
        .map(|(done, changed)| (done.order, changed))
    }

    fn order(&self) -> Order {
        block_on(self.orders.get(order_id())).unwrap().unwrap()
    }

    fn item(&self) -> Option<InventoryItem> {
        block_on(self.inventory_items.get(mug_item())).unwrap()
    }

    /// (on hand at the warehouse, at the shop, reserved for the mugs line by location).
    fn stock(&self) -> (i64, i64, Vec<(LocationId, u32)>) {
        let item = self.item().unwrap();
        let held = item
            .reservations()
            .iter()
            .filter(|r| r.reference == reference(mugs()))
            .map(|r| (r.location, r.quantity.value()))
            .collect();
        (item.on_hand(warehouse()), item.on_hand(shop()), held)
    }

    fn view(&self) -> OrderView {
        block_on(order_view(&Ctx::system(), &self.ports, order_id())).unwrap()
    }
}

fn reference(line: OrderLineId) -> ReservationRef {
    ReservationRef::new(&line.to_string()).unwrap()
}

fn user_with(scopes: Vec<Scope>) -> Ctx {
    Ctx::new(Actor::User(UserId::from_uuid(Uuid::from_u128(9))), scopes)
}

fn key(key: &str, fingerprint: u8) -> Option<IdempotencyRequest> {
    Some(IdempotencyRequest {
        key: key.to_string(),
        fingerprint: [fingerprint; 32],
    })
}

// ---- fulfil ----

#[test]
fn fulfilling_everything_ships_the_order_and_takes_the_stock() {
    let world = World::new();
    let done = world
        .fulfil(warehouse(), &[(mugs(), 3), (ebook(), 1)])
        .unwrap();
    assert_eq!(
        done.order.fulfillment_status(),
        FulfillmentStatus::Fulfilled
    );
    assert_eq!(done.order, world.order());
    assert_eq!(
        block_on(world.fulfillments.get(done.fulfillment.id())),
        Ok(Some(done.fulfillment.clone()))
    );
    assert_eq!(done.fulfillment.location(), warehouse());
    assert_eq!(done.fulfillment.tracking()[0].number(), "JD0001");
    // The reservation is used up, and the units are gone from the warehouse.
    assert_eq!(world.stock(), (7, 5, vec![]));
}

#[test]
fn fulfilling_writes_its_events_with_the_stock_change() {
    let world = World::new();
    world.fulfil(warehouse(), &[(mugs(), 1)]).unwrap();
    let types = world.outbox.types();
    assert_eq!(
        types[types.len() - 3..],
        [
            "fulfillment.created",
            "order.fulfillment_status_changed",
            "inventory_item.changed"
        ]
    );
}

#[test]
fn fulfilling_part_leaves_the_rest_reserved() {
    let world = World::new();
    let done = world.fulfil(warehouse(), &[(mugs(), 2)]).unwrap();
    assert_eq!(
        done.order.fulfillment_status(),
        FulfillmentStatus::PartiallyFulfilled
    );
    assert_eq!(world.stock(), (8, 5, vec![(warehouse(), 1)]));
}

#[test]
fn fulfilling_from_another_location_releases_the_reservation() {
    let world = World::new();
    world.fulfil(shop(), &[(mugs(), 3)]).unwrap();
    assert_eq!(world.stock(), (10, 2, vec![]));
}

#[test]
fn an_untracked_line_ships_without_touching_stock() {
    let world = World::new();
    let done = world.fulfil(warehouse(), &[(ebook(), 1)]).unwrap();
    assert_eq!(done.order.lines()[1].fulfilled_quantity, 1);
    assert_eq!(world.stock(), (10, 5, vec![(warehouse(), 3)]));
}

#[test]
fn more_than_is_left_is_refused_and_nothing_is_written() {
    let world = World::new();
    world.fulfil(warehouse(), &[(mugs(), 2)]).unwrap();
    let before = world.outbox.types().len();
    assert_eq!(
        world.fulfil(warehouse(), &[(ebook(), 1), (mugs(), 2)]),
        Err(CommandError::Order(OrderError::OverFulfilled {
            line: mugs(),
            requested: 2,
            unfulfilled: 1
        }))
    );
    assert_eq!(world.outbox.types().len(), before);
    assert_eq!(world.order().lines()[1].fulfilled_quantity, 0);
    assert_eq!(world.stock(), (8, 5, vec![(warehouse(), 1)]));
}

#[test]
fn a_line_listed_twice_is_refused() {
    let world = World::new();
    assert_eq!(
        world.fulfil(warehouse(), &[(mugs(), 1), (mugs(), 1)]),
        Err(CommandError::Fulfillment(FulfillmentError::DuplicateLine(
            mugs()
        )))
    );
}

#[test]
fn bad_input_is_refused() {
    let world = World::new();
    let other = OrderLineId::from_uuid(Uuid::from_u128(9999));
    assert_eq!(
        world.fulfil(warehouse(), &[(other, 1)]),
        Err(CommandError::Order(OrderError::UnknownLine(other)))
    );
    assert_eq!(
        world.fulfil(warehouse(), &[(mugs(), 0)]),
        Err(CommandError::Quantity(QuantityError::Zero))
    );
    let nowhere = LocationId::from_uuid(Uuid::from_u128(9999));
    assert_eq!(
        world.fulfil(nowhere, &[(mugs(), 1)]),
        Err(CommandError::LocationNotFound(nowhere))
    );
    assert_eq!(
        world.fulfil(warehouse(), &[]),
        Err(CommandError::Fulfillment(FulfillmentError::NoLines))
    );
}

#[test]
fn an_unknown_order_is_reported() {
    let world = World::new();
    let input = Fulfil {
        order: OrderId::from_uuid(Uuid::from_u128(9999)),
        location: warehouse(),
        lines: vec![FulfilLine {
            line: mugs(),
            quantity: 1,
        }],
        tracking: vec![],
        notify_customer: false,
        idempotency: None,
    };
    assert_eq!(
        block_on(fulfill(
            &Ctx::system(),
            &world.ports,
            &world.ids,
            &world.clock,
            input
        )),
        Err(CommandError::OrderNotFound(OrderId::from_uuid(
            Uuid::from_u128(9999)
        )))
    );
}

#[test]
fn whether_to_tell_the_buyer_travels_in_the_events() {
    let world = World::new();
    let input = Fulfil {
        order: order_id(),
        location: warehouse(),
        lines: vec![FulfilLine {
            line: mugs(),
            quantity: 1,
        }],
        tracking: vec![],
        notify_customer: true,
        idempotency: None,
    };
    block_on(fulfill(
        &Ctx::system(),
        &world.ports,
        &world.ids,
        &world.clock,
        input,
    ))
    .unwrap();
    let created = world
        .outbox
        .events()
        .into_iter()
        .find(|e| e.event_type == "fulfillment.created")
        .unwrap();
    assert_eq!(created.data["notify_customer"], true);

    let world = World::new();
    let input = CancelOrder {
        order: order_id(),
        reason: CancelReason::Customer,
        notify_customer: true,
    };
    block_on(cancel_order(
        &Ctx::system(),
        &world.ports,
        &world.clock,
        input,
    ))
    .unwrap();
    let cancelled = world
        .outbox
        .events()
        .into_iter()
        .find(|e| e.event_type == "order.cancelled")
        .unwrap();
    assert_eq!(cancelled.data["notify_customer"], true);
    assert_eq!(cancelled.data["reason"], "customer");
}

#[test]
fn a_cancelled_order_cant_be_fulfilled() {
    let world = World::new();
    world.cancel(CancelReason::Customer).unwrap();
    assert_eq!(
        world.fulfil(warehouse(), &[(mugs(), 1)]),
        Err(CommandError::Order(OrderError::Cancelled(order_id())))
    );
}

#[test]
fn fulfilling_needs_fulfillments_write() {
    let world = World::new();
    // Changing orders isn't shipping them (FU9).
    let ctx = user_with(vec![Scope::OrdersWrite]);
    assert_eq!(
        world
            .fulfil_as(&ctx, warehouse(), &[(mugs(), 1)], None)
            .map(|d| d.value),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::FulfillmentsWrite
        )))
    );
}

#[test]
fn retried_with_its_key_fulfilling_replays_the_first_fulfilment() {
    let world = World::new();
    let first = world
        .fulfil_as(&Ctx::system(), warehouse(), &[(mugs(), 1)], key("k", 1))
        .unwrap();
    let again = world
        .fulfil_as(&Ctx::system(), warehouse(), &[(mugs(), 1)], key("k", 1))
        .unwrap();
    assert!(!first.replayed && again.replayed);
    assert_eq!(again.value, first.value);
    assert_eq!(world.order().lines()[0].fulfilled_quantity, 1);
    // The key with another body is a client bug.
    assert_eq!(
        world
            .fulfil_as(&Ctx::system(), warehouse(), &[(mugs(), 2)], key("k", 2))
            .map(|d| d.value),
        Err(CommandError::Idempotency(IdempotencyError::KeyReused))
    );
}

#[test]
fn a_retry_that_loses_the_race_to_its_own_key_replays() {
    let world = World::new();
    let first = world
        .fulfil_as(&Ctx::system(), warehouse(), &[(mugs(), 3)], key("k", 1))
        .unwrap();
    // The retry's first look misses the record, so it runs, finds nothing left to
    // fulfil, and then finds the record.
    world.idempotency.hide_next_lookups(1);
    let again = world
        .fulfil_as(&Ctx::system(), warehouse(), &[(mugs(), 3)], key("k", 1))
        .unwrap();
    assert!(again.replayed);
    assert_eq!(again.value.fulfillment.id(), first.value.fulfillment.id());
}

#[test]
fn the_last_reservation_of_a_retired_item_takes_the_item_with_it() {
    let world = World::new();
    let mut item = world.item().unwrap();
    item.retire();
    block_on(world.inventory_items.update(&mut item)).unwrap();
    world.fulfil(warehouse(), &[(mugs(), 2)]).unwrap();
    assert!(world.item().is_some());
    world.fulfil(warehouse(), &[(mugs(), 1)]).unwrap();
    assert_eq!(world.item(), None);
    assert_eq!(
        world.outbox.types().last().unwrap(),
        "inventory_item.deleted"
    );
}

// ---- cancel a fulfilment ----

fn shipped_two(world: &World) -> FulfillmentId {
    world
        .fulfil(warehouse(), &[(mugs(), 2), (ebook(), 1)])
        .unwrap()
        .fulfillment
        .id()
}

#[test]
fn cancelling_a_fulfilment_puts_its_units_back() {
    let world = World::new();
    let id = shipped_two(&world);
    let done = block_on(cancel_fulfillment(
        &Ctx::system(),
        &world.ports,
        &world.clock,
        id,
    ))
    .unwrap();
    assert_eq!(
        done.fulfillment.state(),
        FulfillmentState::Cancelled {
            at: FixedClock::DEFAULT
        }
    );
    assert_eq!(
        done.order.fulfillment_status(),
        FulfillmentStatus::Unfulfilled
    );
    assert_eq!(done.order, world.order());
    // Back on hand and reserved again at the warehouse.
    assert_eq!(world.stock(), (10, 5, vec![(warehouse(), 3)]));
    assert!(
        world
            .outbox
            .types()
            .contains(&"fulfillment.cancelled".to_string())
    );
}

#[test]
fn cancelling_a_fulfilment_again_changes_nothing() {
    let world = World::new();
    let id = shipped_two(&world);
    block_on(cancel_fulfillment(
        &Ctx::system(),
        &world.ports,
        &world.clock,
        id,
    ))
    .unwrap();
    let events = world.outbox.types().len();
    let again = block_on(cancel_fulfillment(
        &Ctx::system(),
        &world.ports,
        &world.clock,
        id,
    ))
    .unwrap();
    assert_eq!(again.fulfillment.version().value(), 2);
    assert_eq!(world.outbox.types().len(), events);
    assert_eq!(world.stock(), (10, 5, vec![(warehouse(), 3)]));
}

#[test]
fn cancelling_an_unknown_fulfilment_is_reported() {
    let world = World::new();
    let id = FulfillmentId::from_uuid(Uuid::from_u128(9999));
    assert_eq!(
        block_on(cancel_fulfillment(
            &Ctx::system(),
            &world.ports,
            &world.clock,
            id
        )),
        Err(CommandError::FulfillmentNotFound(id))
    );
}

#[test]
fn cancelling_a_fulfilment_needs_fulfillments_write() {
    let world = World::new();
    let id = shipped_two(&world);
    let ctx = user_with(vec![Scope::OrdersWrite]);
    assert_eq!(
        block_on(cancel_fulfillment(&ctx, &world.ports, &world.clock, id)),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::FulfillmentsWrite
        )))
    );
}

// ---- cancel an order ----

#[test]
fn cancelling_an_order_releases_its_stock() {
    let world = World::new();
    let order = world.cancel(CancelReason::Inventory).unwrap();
    assert_eq!(
        order.cancelled().map(|c| c.reason),
        Some(CancelReason::Inventory)
    );
    assert_eq!(order, world.order());
    assert_eq!(world.stock(), (10, 5, vec![]));
    let types = world.outbox.types();
    assert_eq!(
        types[types.len() - 2..],
        ["order.cancelled", "inventory_item.changed"]
    );
}

#[test]
fn an_order_with_standing_fulfilments_cant_be_cancelled() {
    let world = World::new();
    let id = shipped_two(&world);
    assert_eq!(
        world.cancel(CancelReason::Customer),
        Err(CommandError::HasFulfillments {
            order: order_id(),
            fulfillments: vec![id]
        })
    );
    block_on(cancel_fulfillment(
        &Ctx::system(),
        &world.ports,
        &world.clock,
        id,
    ))
    .unwrap();
    assert!(world.cancel(CancelReason::Customer).is_ok());
    assert_eq!(world.stock(), (10, 5, vec![]));
}

#[test]
fn cancelling_again_keeps_the_first_cancellation() {
    let world = World::new();
    world.cancel(CancelReason::Customer).unwrap();
    let events = world.outbox.types().len();
    let again = world.cancel(CancelReason::Fraud).unwrap();
    assert_eq!(
        again.cancelled().map(|c| c.reason),
        Some(CancelReason::Customer)
    );
    assert_eq!(world.outbox.types().len(), events);
}

#[test]
fn a_paid_order_cancelled_stays_paid() {
    let world = World::new();
    let mut order = world.order();
    order.set_financial_status(FinancialStatus::Paid);
    block_on(world.orders.update(&mut order)).unwrap();
    let cancelled = world.cancel(CancelReason::Customer).unwrap();
    assert_eq!(cancelled.financial_status(), FinancialStatus::Paid);
}

#[test]
fn cancelling_releases_a_retired_item_and_deletes_it() {
    let world = World::new();
    let mut item = world.item().unwrap();
    item.retire();
    block_on(world.inventory_items.update(&mut item)).unwrap();
    world.cancel(CancelReason::Other).unwrap();
    assert_eq!(world.item(), None);
}

#[test]
fn cancelling_an_order_needs_orders_write() {
    let world = World::new();
    let ctx = user_with(vec![Scope::FulfillmentsWrite]);
    let input = CancelOrder {
        order: order_id(),
        reason: CancelReason::Staff,
        notify_customer: false,
    };
    assert_eq!(
        block_on(cancel_order(&ctx, &world.ports, &world.clock, input)),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::OrdersWrite
        )))
    );
}

// ---- what's left to ship ----

#[test]
fn whats_left_is_where_it_was_reserved() {
    let world = World::new();
    assert_eq!(
        world.view().unfulfilled,
        [
            Unfulfilled {
                line: mugs(),
                location: Some(warehouse()),
                quantity: 3
            },
            Unfulfilled {
                line: ebook(),
                location: None,
                quantity: 1
            },
        ]
    );
    world
        .fulfil(warehouse(), &[(mugs(), 2), (ebook(), 1)])
        .unwrap();
    assert_eq!(
        world.view().unfulfilled,
        [Unfulfilled {
            line: mugs(),
            location: Some(warehouse()),
            quantity: 1
        }]
    );
}

#[test]
fn a_split_reservation_shows_at_each_location() {
    let world = World::new();
    // Re-reserve the mugs with 1 at the warehouse and 2 at the shop: neither covers
    // all 3, so the line is split (design note 0004, I9).
    let mut item = world.item().unwrap();
    item.release(&reference(mugs()));
    item.set_on_hand(warehouse(), 1, None).unwrap();
    item.set_on_hand(shop(), 2, None).unwrap();
    let locations = block_on(world.locations.serving(world.order().market())).unwrap();
    item.reserve(&reference(mugs()), Quantity::new(3).unwrap(), &locations)
        .unwrap();
    block_on(world.inventory_items.update(&mut item)).unwrap();
    let mugs_left: Vec<(Option<LocationId>, u32)> = world
        .view()
        .unfulfilled
        .iter()
        .filter(|u| u.line == mugs())
        .map(|u| (u.location, u.quantity))
        .collect();
    assert_eq!(mugs_left, [(Some(warehouse()), 1), (Some(shop()), 2)]);
}

#[test]
fn a_cancelled_order_has_nothing_left() {
    let world = World::new();
    world.cancel(CancelReason::Customer).unwrap();
    assert_eq!(world.view().unfulfilled, []);
}

#[test]
fn the_view_needs_orders_read() {
    let world = World::new();
    let ctx = user_with(vec![Scope::FulfillmentsRead]);
    assert_eq!(
        block_on(order_view(&ctx, &world.ports, order_id())).map(|v| v.order.id()),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::OrdersRead
        )))
    );
}

// ---- review fixes ----

fn set_tracked(world: &World, tracked: bool) {
    let mut item = world.item().unwrap();
    item.set_tracked(tracked);
    block_on(world.inventory_items.update(&mut item)).unwrap();
}

fn take_back(world: &World, id: FulfillmentId) -> Fulfilled {
    block_on(cancel_fulfillment(
        &Ctx::system(),
        &world.ports,
        &world.clock,
        id,
    ))
    .unwrap()
}

#[test]
fn a_fulfilment_notes_which_lines_took_stock() {
    let world = World::new();
    let done = world
        .fulfil(warehouse(), &[(mugs(), 1), (ebook(), 1)])
        .unwrap();
    let taken: Vec<bool> = done
        .fulfillment
        .lines()
        .iter()
        .map(|l| l.stock_taken)
        .collect();
    // The mugs are tracked; the e-book has no item.
    assert_eq!(taken, [true, false]);
}

#[test]
fn taking_back_what_shipped_untracked_puts_no_stock_back() {
    let world = World::new();
    set_tracked(&world, false);
    let id = world
        .fulfil(warehouse(), &[(mugs(), 2)])
        .unwrap()
        .fulfillment
        .id();
    assert_eq!(world.item().unwrap().on_hand(warehouse()), 10);
    // Tracking is turned on before the shipment is taken back: no units appear.
    set_tracked(&world, true);
    take_back(&world, id);
    let item = world.item().unwrap();
    assert_eq!(item.on_hand(warehouse()), 10);
}

#[test]
fn taking_back_what_shipped_tracked_restores_the_count_even_if_untracked_since() {
    let world = World::new();
    let id = world
        .fulfil(warehouse(), &[(mugs(), 2)])
        .unwrap()
        .fulfillment
        .id();
    assert_eq!(world.item().unwrap().on_hand(warehouse()), 8);
    set_tracked(&world, false);
    take_back(&world, id);
    // The count is right for when tracking comes back; untracked items reserve nothing.
    assert_eq!(world.stock(), (10, 5, vec![(warehouse(), 1)]));
}

#[test]
fn only_the_request_that_cancels_says_it_changed_the_order() {
    let world = World::new();
    assert!(world.cancel_and_changed(CancelReason::Customer).unwrap().1);
    assert!(!world.cancel_and_changed(CancelReason::Customer).unwrap().1);
}

// ---- payments (design note 0027) ----

/// A pending payment of the order's total, as checkout leaves it.
fn pending_payment(world: &World) -> Payment {
    let mut payment = Payment::new(NewPayment {
        id: hypha_core::PaymentId::from_uuid(Uuid::from_u128(8001)),
        order: order_id(),
        method: Some(hypha_core::PaymentMethodId::from_uuid(Uuid::from_u128(
            8002,
        ))),
        method_name: Some("Bank transfer".to_string()),
        provider: ProviderKey::manual(),
        amount: eur(2000),
        instructions: Some("IBAN HR12".to_string()),
    })
    .unwrap();
    block_on(world.payments.insert(&mut payment)).unwrap();
    payment
}

impl World {
    fn mark_paid_as(
        &self,
        ctx: &Ctx,
        reference: Option<&str>,
    ) -> Result<OrderPayments, CommandError> {
        let input = MarkPaid {
            order: order_id(),
            reference: reference.map(str::to_string),
        };
        block_on(mark_paid(ctx, &self.ports, &self.ids, &self.clock, input))
    }

    fn mark_paid(&self, reference: Option<&str>) -> Result<OrderPayments, CommandError> {
        self.mark_paid_as(&Ctx::system(), reference)
    }
}

#[test]
fn marking_paid_records_the_pending_payment() {
    let world = World::new();
    let pending = pending_payment(&world);
    let done = world.mark_paid(Some("RF-1001")).unwrap();
    assert_eq!(done.order.financial_status(), FinancialStatus::Paid);
    assert_eq!(done.payments.len(), 1);
    let paid = &done.payments[0];
    assert_eq!(paid.id(), pending.id());
    assert_eq!(
        (paid.status(), paid.reference(), paid.paid_at()),
        (
            PaymentStatus::Paid,
            Some("RF-1001"),
            Some(FixedClock::DEFAULT)
        )
    );
    assert_eq!(
        block_on(world.payments.get(paid.id())),
        Ok(Some(paid.clone()))
    );
    assert_eq!(world.order(), done.order);
    let types = world.outbox.types();
    let tail = &types[types.len() - 2..];
    assert!(tail.contains(&"payment.paid".to_string()) && tail.contains(&"order.paid".to_string()));
}

#[test]
fn marking_an_order_without_a_payment_creates_a_manual_one() {
    let world = World::new();
    let done = world.mark_paid(None).unwrap();
    assert_eq!(done.order.financial_status(), FinancialStatus::Paid);
    let [payment] = done.payments.as_slice() else {
        panic!("one payment: {:?}", done.payments)
    };
    assert_eq!(payment.provider(), &ProviderKey::manual());
    assert_eq!((payment.method(), payment.method_name()), (None, None));
    assert_eq!(payment.amount(), eur(2000));
    assert_eq!(payment.status(), PaymentStatus::Paid);
    assert_eq!(
        block_on(world.payments.list_for_order(order_id())),
        Ok(done.payments.clone())
    );
}

#[test]
fn marking_a_paid_order_again_changes_nothing() {
    let world = World::new();
    pending_payment(&world);
    let first = world.mark_paid(Some("first")).unwrap();
    let events = world.outbox.types().len();
    let again = world.mark_paid(Some("second")).unwrap();
    assert_eq!(again, first);
    assert_eq!(world.outbox.types().len(), events);
}

#[test]
fn a_cancelled_order_cant_be_marked_paid() {
    let world = World::new();
    pending_payment(&world);
    world.cancel(CancelReason::Fraud).unwrap();
    assert_eq!(
        world.mark_paid(None),
        Err(CommandError::Order(OrderError::Cancelled(order_id())))
    );
}

#[test]
fn a_reference_is_short_text() {
    let world = World::new();
    pending_payment(&world);
    assert_eq!(
        world.mark_paid(Some(&"x".repeat(256))),
        Err(CommandError::Payment(PaymentError::Text(
            hypha_core::TextError::TooLong("reference")
        )))
    );
    assert_eq!(world.order().financial_status(), FinancialStatus::Pending);
}

#[test]
fn marking_paid_needs_orders_write() {
    let world = World::new();
    assert_eq!(
        world.mark_paid_as(&user_with(vec![Scope::PaymentsWrite]), None),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::OrdersWrite
        )))
    );
}

#[test]
fn cancelling_voids_the_pending_payment() {
    let world = World::new();
    let pending = pending_payment(&world);
    let cancelled = world.cancel(CancelReason::Customer).unwrap();
    assert_eq!(cancelled.financial_status(), FinancialStatus::Voided);
    let voided = block_on(world.payments.get(pending.id())).unwrap().unwrap();
    assert_eq!(
        (voided.status(), voided.voided_at()),
        (PaymentStatus::Voided, Some(FixedClock::DEFAULT))
    );
    assert!(world.outbox.types().contains(&"payment.voided".to_string()));
    // The view shows it.
    assert_eq!(world.view().payments, [voided]);
}

#[test]
fn cancelling_a_paid_order_leaves_its_payment_paid() {
    let world = World::new();
    pending_payment(&world);
    world.mark_paid(None).unwrap();
    let cancelled = world.cancel(CancelReason::Customer).unwrap();
    assert_eq!(cancelled.financial_status(), FinancialStatus::Paid);
    let payments = block_on(world.payments.list_for_order(order_id())).unwrap();
    assert_eq!(payments[0].status(), PaymentStatus::Paid);
}

#[test]
fn cancelling_an_order_without_payments_leaves_it_pending() {
    let world = World::new();
    let cancelled = world.cancel(CancelReason::Customer).unwrap();
    assert_eq!(cancelled.financial_status(), FinancialStatus::Pending);
}

/// Payment `n` of the order, stored as `status` with `refunded` given back (built
/// straight from a snapshot: providers that authorize and refund don't exist yet), and
/// the order's payment status following its payments.
fn stored_payment(world: &World, n: u128, amount: i64, status: PaymentStatus, refunded: i64) {
    let mut payment = Payment::restore(PaymentSnapshot {
        id: hypha_core::PaymentId::from_uuid(Uuid::from_u128(8100 + n)),
        version: hypha_core::Version::NEW,
        stamps: None,
        order: order_id(),
        method: None,
        method_name: None,
        provider: ProviderKey::manual(),
        amount: eur(amount),
        status,
        refunded: eur(refunded),
        reference: None,
        refund_references: vec![],
        attempt: None,
        instructions: None,
        paid_at: status.captured().then_some(FixedClock::DEFAULT),
        voided_at: (status == PaymentStatus::Voided).then_some(FixedClock::DEFAULT),
    })
    .unwrap();
    block_on(world.payments.insert(&mut payment)).unwrap();
    let payments = block_on(world.payments.list_for_order(order_id())).unwrap();
    let mut order = world.order();
    order.set_financial_status(hypha_payments::financial_status(order.total(), &payments).unwrap());
    block_on(world.orders.update(&mut order)).unwrap();
}

#[test]
fn marking_paid_records_every_pending_payment() {
    let world = World::new();
    stored_payment(&world, 1, 1200, PaymentStatus::Pending, 0);
    stored_payment(&world, 2, 800, PaymentStatus::Pending, 0);
    let done = world.mark_paid(Some("RF-1")).unwrap();
    assert_eq!(done.order.financial_status(), FinancialStatus::Paid);
    assert_eq!(done.payments.len(), 2, "no manual payment added");
    for p in &done.payments {
        assert_eq!(
            (p.status(), p.reference()),
            (PaymentStatus::Paid, Some("RF-1"))
        );
    }
    assert_eq!(
        block_on(world.payments.list_for_order(order_id())),
        Ok(done.payments)
    );
}

#[test]
fn marking_pending_payments_paid_besides_one_already_paid() {
    // Partially paid: part collected, the rest still pending.
    let world = World::new();
    stored_payment(&world, 1, 1200, PaymentStatus::Paid, 0);
    stored_payment(&world, 2, 800, PaymentStatus::Pending, 0);
    assert_eq!(
        world.order().financial_status(),
        FinancialStatus::PartiallyPaid
    );
    let done = world.mark_paid(None).unwrap();
    assert_eq!(done.order.financial_status(), FinancialStatus::Paid);
    assert_eq!(done.payments.len(), 2);
}

#[test]
fn an_authorized_payment_isnt_marked_paid_by_hand() {
    for others in [vec![], vec![PaymentStatus::Pending]] {
        let world = World::new();
        stored_payment(&world, 1, 2000, PaymentStatus::Authorized, 0);
        for (n, status) in others.iter().enumerate() {
            stored_payment(&world, 2 + n as u128, 2000, *status, 0);
        }
        let before = world.order();
        let events = world.outbox.types().len();
        assert_eq!(
            world.mark_paid(None),
            Err(CommandError::PaymentAuthorized {
                order: order_id(),
                payment: hypha_core::PaymentId::from_uuid(Uuid::from_u128(8101)),
            })
        );
        assert_eq!(world.order(), before);
        assert_eq!(world.outbox.types().len(), events);
    }
}

#[test]
fn money_already_moved_isnt_recorded_again() {
    let cases = [
        (PaymentStatus::Paid, 500, 0, FinancialStatus::PartiallyPaid),
        (
            PaymentStatus::PartiallyRefunded,
            2000,
            500,
            FinancialStatus::PartiallyRefunded,
        ),
        (
            PaymentStatus::Refunded,
            2000,
            2000,
            FinancialStatus::Refunded,
        ),
        // Every payment voided, on an order still open.
        (PaymentStatus::Voided, 2000, 0, FinancialStatus::Voided),
    ];
    for (payment, amount, refunded, status) in cases {
        let world = World::new();
        stored_payment(&world, 1, amount, payment, refunded);
        assert_eq!(world.order().financial_status(), status);
        let events = world.outbox.types().len();
        assert_eq!(
            world.mark_paid(None),
            Err(CommandError::NotPayable {
                order: order_id(),
                status
            }),
            "{payment}"
        );
        assert_eq!(world.order().financial_status(), status);
        assert_eq!(
            block_on(world.payments.list_for_order(order_id()))
                .unwrap()
                .len(),
            1,
            "no manual payment for {payment}"
        );
        assert_eq!(world.outbox.types().len(), events);
    }
}

#[test]
fn an_order_whose_payments_failed_gets_a_manual_one() {
    let world = World::new();
    stored_payment(&world, 1, 2000, PaymentStatus::Failed, 0);
    assert_eq!(world.order().financial_status(), FinancialStatus::Pending);
    let done = world.mark_paid(None).unwrap();
    assert_eq!(done.order.financial_status(), FinancialStatus::Paid);
    assert_eq!(done.payments.len(), 2);
    assert_eq!(
        (done.payments[0].provider(), done.payments[0].status()),
        (&ProviderKey::manual(), PaymentStatus::Paid)
    );
}

#[test]
fn marking_paid_twice_at_once_succeeds_both_times() {
    let world = World::new();
    let pending = pending_payment(&world);
    // Both requests load the order and its pending payment; the other one commits first.
    let stale_order = world.order();
    let stale_payments = block_on(world.payments.list_for_order(order_id())).unwrap();
    let first = world.mark_paid(Some("first")).unwrap();
    let events = world.outbox.types().len();

    world.orders.serve_next_get(stale_order);
    world.payments.serve_next_list(stale_payments);
    let second = world.mark_paid(Some("second")).unwrap();
    // The loser's write was stale; it reloaded, found the order paid and left it.
    assert_eq!(second, first);
    assert_eq!(second.order.financial_status(), FinancialStatus::Paid);
    let stored = block_on(world.payments.list_for_order(order_id())).unwrap();
    assert_eq!(stored.len(), 1, "no duplicate manual payment");
    assert_eq!(
        (stored[0].id(), stored[0].status(), stored[0].reference()),
        (pending.id(), PaymentStatus::Paid, Some("first"))
    );
    assert_eq!(world.outbox.types().len(), events);
}

#[test]
fn marking_an_order_without_payments_twice_at_once_records_one() {
    let world = World::new();
    let stale_order = world.order();
    let first = world.mark_paid(None).unwrap();
    // The loser saw no payments: it creates a manual one, its batch fails on the stale
    // order, so that payment is never written.
    world.orders.serve_next_get(stale_order);
    world.payments.serve_next_list(vec![]);
    let second = world.mark_paid(None).unwrap();
    assert_eq!(second, first);
    assert_eq!(
        block_on(world.payments.list_for_order(order_id()))
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn an_order_with_an_authorized_payment_isnt_cancelled() {
    let world = World::new();
    stored_payment(&world, 1, 2000, PaymentStatus::Authorized, 0);
    let before = (world.order(), world.stock());
    assert_eq!(
        world.cancel(CancelReason::Customer),
        Err(CommandError::PaymentAuthorized {
            order: order_id(),
            payment: hypha_core::PaymentId::from_uuid(Uuid::from_u128(8101)),
        })
    );
    assert_eq!((world.order(), world.stock()), before);
}

// ---- payment events (design note 0029, §3.1) ----

impl World {
    fn report(
        &self,
        kind: hypha_order_actions::PaymentEventKind,
        amount: Option<i64>,
        idempotency: Option<IdempotencyRequest>,
    ) -> Result<hypha_core::Idempotent<OrderPayments>, CommandError> {
        use hypha_order_actions::PaymentEventKind as K;
        // A refund's reference is the refund's own ID: none here (staff may leave it).
        let reference = match kind {
            K::PartiallyRefunded | K::Refunded => None,
            _ => Some("pi_1"),
        };
        self.report_ref(kind, amount, reference, idempotency)
    }

    fn report_ref(
        &self,
        kind: hypha_order_actions::PaymentEventKind,
        amount: Option<i64>,
        reference: Option<&str>,
        idempotency: Option<IdempotencyRequest>,
    ) -> Result<hypha_core::Idempotent<OrderPayments>, CommandError> {
        self.report_attempt(kind, amount, reference, None, idempotency)
    }

    /// A report naming its attempt, as a provider whose references name attempts sends.
    fn report_attempt(
        &self,
        kind: hypha_order_actions::PaymentEventKind,
        amount: Option<i64>,
        reference: Option<&str>,
        attempt: Option<&str>,
        idempotency: Option<IdempotencyRequest>,
    ) -> Result<hypha_core::Idempotent<OrderPayments>, CommandError> {
        let input = hypha_order_actions::PaymentEvent {
            payment: hypha_core::PaymentId::from_uuid(Uuid::from_u128(8001)),
            kind,
            amount,
            reference: reference.map(str::to_string),
            attempt: attempt.map(str::to_string),
            occurred_at: None,
            idempotency,
        };
        block_on(hypha_order_actions::record_payment_event(
            &Ctx::system(),
            &self.ports,
            &self.clock,
            input,
        ))
    }
}

#[test]
fn an_authorization_then_a_capture_move_the_payment_and_the_order() {
    use hypha_order_actions::PaymentEventKind as K;
    let world = World::new();
    pending_payment(&world);
    let done = world.report(K::Authorized, None, None).unwrap().value;
    assert_eq!(done.payments[0].status(), PaymentStatus::Authorized);
    assert_eq!(done.payments[0].reference(), Some("pi_1"));
    assert_eq!(done.order.financial_status(), FinancialStatus::Authorized);
    let done = world.report(K::Paid, Some(2000), None).unwrap().value;
    assert_eq!(done.payments[0].status(), PaymentStatus::Paid);
    assert_eq!(world.order().financial_status(), FinancialStatus::Paid);
    let types = world.outbox.types();
    assert!(
        types.ends_with(&[
            "payment.authorized".to_string(),
            "payment.paid".to_string(),
            "order.paid".to_string()
        ]),
        "{types:?}"
    );
}

#[test]
fn a_report_repeated_changes_nothing() {
    use hypha_order_actions::PaymentEventKind as K;
    let world = World::new();
    pending_payment(&world);
    world.report(K::Paid, None, None).unwrap();
    let before = world.outbox.types().len();
    let again = world.report(K::Paid, None, None).unwrap().value;
    assert_eq!(again.payments[0].status(), PaymentStatus::Paid);
    assert_eq!(world.outbox.types().len(), before);
}

#[test]
fn a_report_with_its_key_replays() {
    use hypha_order_actions::PaymentEventKind as K;
    let world = World::new();
    pending_payment(&world);
    world.report(K::Paid, None, None).unwrap();
    let first = world
        .report(K::PartiallyRefunded, Some(500), key("refund-1", 1))
        .unwrap();
    assert!(!first.replayed);
    let again = world
        .report(K::PartiallyRefunded, Some(500), key("refund-1", 1))
        .unwrap();
    assert!(again.replayed);
    assert_eq!(again.value.payments[0].refunded(), eur(500));
    assert_eq!(
        again.value.order.financial_status(),
        FinancialStatus::PartiallyRefunded
    );
    // The rest, then.
    let done = world.report(K::Refunded, None, None).unwrap().value;
    assert_eq!(done.payments[0].refunded(), eur(2000));
    assert_eq!(done.order.financial_status(), FinancialStatus::Refunded);
}

#[test]
fn a_refund_reported_again_under_its_reference_counts_once() {
    use hypha_order_actions::PaymentEventKind as K;
    let world = World::new();
    pending_payment(&world);
    world.report(K::Paid, None, None).unwrap();
    let first = world
        .report_ref(K::PartiallyRefunded, Some(500), Some("re_1"), None)
        .unwrap()
        .value;
    assert_eq!(first.payments[0].refunded(), eur(500));
    let events = world.outbox.types().len();
    // Delivered twice by the provider, reported under another key (or none): nothing.
    let again = world
        .report_ref(
            K::PartiallyRefunded,
            Some(500),
            Some("re_1"),
            key("other", 2),
        )
        .unwrap()
        .value;
    assert_eq!(again.payments[0].refunded(), eur(500));
    assert_eq!(world.outbox.types().len(), events);
    // Another refund counts; the payment keeps its own reference.
    let done = world
        .report_ref(K::Refunded, Some(1500), Some("re_2"), None)
        .unwrap()
        .value;
    assert_eq!(done.payments[0].refunded(), eur(2000));
    assert_eq!(done.payments[0].reference(), Some("pi_1"));
    assert_eq!(done.payments[0].refund_references(), ["re_1", "re_2"]);
    assert_eq!(done.order.financial_status(), FinancialStatus::Refunded);
    // Refunded wholly, the first refund arriving late again is still nothing.
    let late = world
        .report_ref(K::PartiallyRefunded, Some(500), Some("re_1"), None)
        .unwrap()
        .value;
    assert_eq!(late.payments[0].refunded(), eur(2000));
}

#[test]
fn an_app_must_name_the_refund_it_reports() {
    use hypha_order_actions::PaymentEventKind as K;
    let world = World::new();
    app_payment(&world, "app_pay", app_id(71));
    let app = Actor::App(app_id(71));
    assert_eq!(world.report_as(app, K::Paid), Ok(PaymentStatus::Paid));
    assert_eq!(
        world.report_as(app, K::Refunded),
        Err(CommandError::RefundReferenceRequired(
            hypha_core::PaymentId::from_uuid(Uuid::from_u128(8001))
        ))
    );
    // Staff may record a refund without the provider's ID.
    let staff = Actor::User(hypha_core::UserId::from_uuid(Uuid::from_u128(9)));
    assert_eq!(
        world.report_as(staff, K::Refunded),
        Ok(PaymentStatus::Refunded)
    );
}

#[test]
fn a_report_naming_another_attempt_is_refused_once_the_payment_is_past_pending() {
    use hypha_order_actions::PaymentEventKind as K;
    let world = World::new();
    pending_payment(&world);
    world
        .report_attempt(K::Authorized, None, Some("pi_1"), Some("pi_1"), None)
        .unwrap();
    let mismatch = |status| {
        Err(CommandError::Payment(PaymentError::ReferenceMismatch {
            payment: hypha_core::PaymentId::from_uuid(Uuid::from_u128(8001)),
            status,
            held: "pi_1".to_string(),
            reported: "pi_9".to_string(),
        }))
    };
    // Another attempt's money, decline, cancel: none of them the payment's.
    for kind in [K::Paid, K::Authorized, K::Failed, K::Voided] {
        assert_eq!(
            world
                .report_attempt(kind, None, Some("pi_9"), Some("pi_9"), None)
                .map(|_| ()),
            mismatch(PaymentStatus::Authorized),
            "{kind:?}"
        );
    }
    // Its own capture, then another attempt's refund.
    world
        .report_attempt(K::Paid, None, Some("pi_1"), Some("pi_1"), None)
        .unwrap();
    assert_eq!(
        world
            .report_attempt(
                K::PartiallyRefunded,
                Some(500),
                Some("re_9"),
                Some("pi_9"),
                None
            )
            .map(|_| ()),
        mismatch(PaymentStatus::Paid)
    );
    assert_eq!(world.order().financial_status(), FinancialStatus::Paid);
}

#[test]
fn a_pending_payment_is_paid_through_whichever_attempt_and_keeps_it() {
    use hypha_order_actions::PaymentEventKind as K;
    let world = World::new();
    pending_payment(&world);
    // Started twice (pi_1, then pi_9); the old tab's pi_1 is paid first.
    let paid = world
        .report_attempt(K::Paid, None, Some("pi_1"), Some("pi_1"), None)
        .unwrap()
        .value;
    assert_eq!(paid.payments[0].status(), PaymentStatus::Paid);
    assert_eq!(paid.payments[0].attempt(), Some("pi_1"));
    // Then pi_9's money: a second collection.
    assert_eq!(
        world
            .report_attempt(K::Paid, None, Some("pi_9"), Some("pi_9"), None)
            .map(|_| ()),
        Err(CommandError::Payment(PaymentError::ReferenceMismatch {
            payment: hypha_core::PaymentId::from_uuid(Uuid::from_u128(8001)),
            status: PaymentStatus::Paid,
            held: "pi_1".to_string(),
            reported: "pi_9".to_string(),
        }))
    );
}

#[test]
fn a_restarted_pending_payment_refuses_its_old_attempts_cancel() {
    use hypha_order_actions::PaymentEventKind as K;
    let world = World::new();
    // Its start made pi_1, then a start made again pi_9 (`Started::attempt`).
    let mut payment = pending_payment(&world);
    payment.note_attempt(Some("pi_9".to_string())).unwrap();
    block_on(world.payments.update(&mut payment)).unwrap();
    assert_eq!(
        world
            .report_attempt(K::Voided, None, Some("pi_1"), Some("pi_1"), None)
            .map(|_| ()),
        Err(CommandError::Payment(PaymentError::ReferenceMismatch {
            payment: payment.id(),
            status: PaymentStatus::Pending,
            held: "pi_9".to_string(),
            reported: "pi_1".to_string(),
        }))
    );
    // pi_9 is the one that's paid.
    let paid = world
        .report_attempt(K::Paid, None, Some("pi_9"), Some("pi_9"), None)
        .unwrap()
        .value;
    assert_eq!(paid.payments[0].status(), PaymentStatus::Paid);
}

#[test]
fn a_paid_payment_without_an_attempt_refuses_a_second_collection() {
    use hypha_order_actions::PaymentEventKind as K;
    let world = World::new();
    pending_payment(&world);
    world.report_ref(K::Paid, None, Some("pi_1"), None).unwrap();
    // A second intent's money: held to the reference, refused, so it goes back.
    assert_eq!(
        world
            .report_attempt(K::Paid, None, Some("pi_2"), Some("pi_2"), None)
            .map(|_| ()),
        Err(CommandError::Payment(PaymentError::ReferenceMismatch {
            payment: hypha_core::PaymentId::from_uuid(Uuid::from_u128(8001)),
            status: PaymentStatus::Paid,
            held: "pi_1".to_string(),
            reported: "pi_2".to_string(),
        }))
    );
}

#[test]
fn a_capture_naming_an_attempt_is_held_to_an_authorization_that_named_none() {
    use hypha_order_actions::PaymentEventKind as K;
    let world = World::new();
    pending_payment(&world);
    world
        .report_ref(K::Authorized, None, Some("AUTH-1"), None)
        .unwrap();
    // Its first report named no attempt: one naming ORDER-9 is held to AUTH-1 (a provider
    // naming attempts names them from the first report).
    assert_eq!(
        world
            .report_attempt(K::Paid, None, Some("CAP-1"), Some("ORDER-9"), None)
            .map(|_| ()),
        Err(CommandError::Payment(PaymentError::ReferenceMismatch {
            payment: hypha_core::PaymentId::from_uuid(Uuid::from_u128(8001)),
            status: PaymentStatus::Authorized,
            held: "AUTH-1".to_string(),
            reported: "ORDER-9".to_string(),
        }))
    );
}

#[test]
fn a_paid_payment_takes_its_own_reference_as_its_attempt_when_named_again() {
    use hypha_order_actions::PaymentEventKind as K;
    let world = World::new();
    pending_payment(&world);
    // Paid before attempts were kept (none named).
    world.report_ref(K::Paid, None, Some("pi_1"), None).unwrap();
    // Stripe delivers it again, naming its intent: kept, though the status is the same.
    world
        .report_attempt(K::Paid, None, Some("pi_1"), Some("pi_1"), None)
        .unwrap();
    let stored = block_on(
        world
            .payments
            .get(hypha_core::PaymentId::from_uuid(Uuid::from_u128(8001))),
    )
    .unwrap()
    .unwrap();
    assert_eq!(stored.attempt(), Some("pi_1"));
}

#[test]
fn an_operations_own_id_doesnt_change_the_attempt() {
    use hypha_order_actions::PaymentEventKind as K;
    let world = World::new();
    pending_payment(&world);
    world
        .report_attempt(K::Authorized, None, Some("pi_1"), Some("pi_1"), None)
        .unwrap();
    // A capture under its own ID (no attempt named): the attempt stays pi_1, so a
    // refund of another attempt is still refused.
    let paid = world
        .report_ref(K::Paid, None, Some("ch_7"), None)
        .unwrap()
        .value;
    assert_eq!(paid.payments[0].reference(), Some("ch_7"));
    assert_eq!(paid.payments[0].attempt(), Some("pi_1"));
    assert_eq!(
        world
            .report_attempt(
                K::PartiallyRefunded,
                Some(500),
                Some("re_9"),
                Some("pi_9"),
                None
            )
            .map(|_| ()),
        Err(CommandError::Payment(PaymentError::ReferenceMismatch {
            payment: hypha_core::PaymentId::from_uuid(Uuid::from_u128(8001)),
            status: PaymentStatus::Paid,
            held: "pi_1".to_string(),
            reported: "pi_9".to_string(),
        }))
    );
}

#[test]
fn a_report_naming_no_attempt_isnt_checked() {
    use hypha_order_actions::PaymentEventKind as K;
    let world = World::new();
    let mut payment = pending_payment(&world);
    payment.set_reference(Some("pi_9".to_string())).unwrap();
    block_on(world.payments.update(&mut payment)).unwrap();
    // An app's report under another reference, naming no attempt: taken.
    let paid = world
        .report_ref(K::Paid, None, Some("pi_1"), None)
        .unwrap()
        .value;
    assert_eq!(paid.payments[0].status(), PaymentStatus::Paid);
    assert_eq!(paid.payments[0].reference(), Some("pi_1"));
}

#[test]
fn a_reference_staff_typed_isnt_an_attempt() {
    use hypha_order_actions::PaymentEventKind as K;
    let world = World::new();
    pending_payment(&world);
    // Staff mark it paid by hand, typing their own reference.
    world
        .mark_paid_as(&Ctx::system(), Some("bank 1234"))
        .unwrap();
    // The provider then reports a refund of its intent: not refused for that text.
    let refunded = world
        .report_attempt(
            K::PartiallyRefunded,
            Some(500),
            Some("re_1"),
            Some("pi_1"),
            None,
        )
        .unwrap()
        .value;
    assert_eq!(refunded.payments[0].refunded(), eur(500));
}

#[test]
fn a_capture_or_void_may_carry_the_providers_own_id_for_it() {
    use hypha_order_actions::PaymentEventKind as K;
    let world = World::new();
    pending_payment(&world);
    world
        .report_ref(K::Authorized, None, Some("AUTH-1"), None)
        .unwrap();
    let paid = world
        .report_ref(K::Paid, None, Some("CAP-7"), None)
        .unwrap()
        .value;
    assert_eq!(paid.payments[0].status(), PaymentStatus::Paid);

    let world = World::new();
    pending_payment(&world);
    world
        .report_ref(K::Authorized, None, Some("AUTH-1"), None)
        .unwrap();
    let voided = world
        .report_ref(K::Voided, None, Some("CANCEL-3"), None)
        .unwrap()
        .value;
    assert_eq!(voided.payments[0].status(), PaymentStatus::Voided);
}

#[test]
fn a_report_the_status_cant_take_is_refused() {
    use hypha_order_actions::PaymentEventKind as K;
    let world = World::new();
    pending_payment(&world);
    world.report(K::Voided, None, None).unwrap();
    assert_eq!(
        world.report(K::Paid, None, None).map(|_| ()),
        Err(CommandError::Payment(PaymentError::InvalidTransition {
            payment: hypha_core::PaymentId::from_uuid(Uuid::from_u128(8001)),
            from: PaymentStatus::Voided,
            to: PaymentStatus::Paid,
        }))
    );
    assert_eq!(world.order().financial_status(), FinancialStatus::Voided);
}

#[test]
fn a_whole_payment_reported_with_another_amount_is_refused() {
    use hypha_order_actions::PaymentEventKind as K;
    let world = World::new();
    pending_payment(&world);
    assert_eq!(
        world.report(K::Paid, Some(1500), None).map(|_| ()),
        Err(CommandError::AmountMismatch {
            payment: hypha_core::PaymentId::from_uuid(Uuid::from_u128(8001)),
            expected: 2000,
            reported: 1500,
        })
    );
}

#[test]
fn reporting_needs_payments_write() {
    let world = World::new();
    pending_payment(&world);
    let input = hypha_order_actions::PaymentEvent {
        payment: hypha_core::PaymentId::from_uuid(Uuid::from_u128(8001)),
        kind: hypha_order_actions::PaymentEventKind::Paid,
        amount: None,
        reference: None,
        attempt: None,
        occurred_at: None,
        idempotency: None,
    };
    assert_eq!(
        block_on(hypha_order_actions::record_payment_event(
            &user_with(vec![Scope::OrdersWrite]),
            &world.ports,
            &world.clock,
            input,
        ))
        .map(|_| ()),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::PaymentsWrite
        )))
    );
}

#[test]
fn an_unknown_payment_is_reported() {
    let world = World::new();
    assert_eq!(
        world
            .report(hypha_order_actions::PaymentEventKind::Paid, None, None)
            .map(|_| ()),
        Err(CommandError::PaymentNotFound(
            hypha_core::PaymentId::from_uuid(Uuid::from_u128(8001))
        ))
    );
}

// ---- who may report (design note 0029, §3.1, §15) ----

fn app_id(n: u128) -> hypha_core::AppId {
    hypha_core::AppId::from_uuid(Uuid::from_u128(n))
}

/// App `key` (ID `id`), and the order's pending payment collected by it.
fn app_payment(world: &World, key: &str, id: hypha_core::AppId) {
    owned_payment(world, key, key, id);
}

/// A payment whose provider is `provider`, and the app `owner` (an app or a spore).
fn owned_payment(world: &World, owner: &str, provider: &str, id: hypha_core::AppId) {
    use hypha_apps::AppRepo;
    let mut app = hypha_apps::App::new(hypha_apps::NewApp {
        key: hypha_core::AppKey::new(owner).unwrap(),
        id,
        name: "Pay".to_string(),
        secret: hypha_core::Sealed::from_bytes(vec![1]),
        webhook: None,
        subscriptions: vec![],
    })
    .unwrap();
    block_on(world.apps.insert(&mut app)).unwrap();
    let mut payment = Payment::new(NewPayment {
        id: hypha_core::PaymentId::from_uuid(Uuid::from_u128(8001)),
        order: order_id(),
        method: None,
        method_name: Some("Card".to_string()),
        provider: ProviderKey::new(provider).unwrap(),
        amount: eur(2000),
        instructions: None,
    })
    .unwrap();
    block_on(world.payments.insert(&mut payment)).unwrap();
}

impl World {
    fn report_as(
        &self,
        actor: Actor,
        kind: hypha_order_actions::PaymentEventKind,
    ) -> Result<PaymentStatus, CommandError> {
        let ctx = Ctx::new(actor, vec![Scope::PaymentsWrite]);
        let input = hypha_order_actions::PaymentEvent {
            payment: hypha_core::PaymentId::from_uuid(Uuid::from_u128(8001)),
            kind,
            amount: None,
            reference: None,
            attempt: None,
            occurred_at: None,
            idempotency: None,
        };
        block_on(hypha_order_actions::record_payment_event(
            &ctx,
            &self.ports,
            &self.clock,
            input,
        ))
        .map(|done| done.value.payments[0].status())
    }
}

#[test]
fn an_app_reports_on_the_payments_it_collects() {
    use hypha_order_actions::PaymentEventKind as K;
    let world = World::new();
    app_payment(&world, "app_pay", app_id(71));
    assert_eq!(
        world.report_as(Actor::App(app_id(71)), K::Paid),
        Ok(PaymentStatus::Paid)
    );
}

#[test]
fn an_app_cant_report_on_another_providers_payment() {
    use hypha_order_actions::PaymentEventKind as K;
    let world = World::new();
    app_payment(&world, "app_pay", app_id(71));
    assert_eq!(
        world.report_as(Actor::App(app_id(72)), K::Paid),
        Err(CommandError::PaymentNotYours {
            payment: hypha_core::PaymentId::from_uuid(Uuid::from_u128(8001)),
            app: app_id(72),
        })
    );
}

#[test]
fn a_spore_reports_on_the_payments_its_providers_collect() {
    use hypha_order_actions::PaymentEventKind as K;
    for provider in ["spore_pay", "spore_pay_link"] {
        let world = World::new();
        owned_payment(&world, "spore_pay", provider, app_id(71));
        assert_eq!(
            world.report_as(Actor::App(app_id(71)), K::Paid),
            Ok(PaymentStatus::Paid),
            "{provider}"
        );
    }
}

#[test]
fn a_spore_cant_report_on_a_payment_under_a_longer_name() {
    use hypha_order_actions::PaymentEventKind as K;
    let world = World::new();
    owned_payment(&world, "spore_pay", "spore_payx", app_id(71));
    assert_eq!(
        world.report_as(Actor::App(app_id(71)), K::Paid),
        Err(CommandError::PaymentNotYours {
            payment: hypha_core::PaymentId::from_uuid(Uuid::from_u128(8001)),
            app: app_id(71),
        })
    );
}

#[test]
fn an_app_cant_report_on_a_manual_payment() {
    use hypha_order_actions::PaymentEventKind as K;
    let world = World::new();
    pending_payment(&world);
    assert_eq!(
        world.report_as(Actor::App(app_id(71)), K::Paid),
        Err(CommandError::PaymentNotYours {
            payment: hypha_core::PaymentId::from_uuid(Uuid::from_u128(8001)),
            app: app_id(71),
        })
    );
}

#[test]
fn staff_report_on_an_apps_payment() {
    use hypha_order_actions::PaymentEventKind as K;
    let world = World::new();
    app_payment(&world, "app_pay", app_id(71));
    let staff = Actor::User(hypha_core::UserId::from_uuid(Uuid::from_u128(9)));
    assert_eq!(world.report_as(staff, K::Paid), Ok(PaymentStatus::Paid));
}
