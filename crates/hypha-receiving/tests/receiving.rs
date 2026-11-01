//! Receiving goods against purchase orders, over a store in memory.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "a failing test panics, and fixtures cast small counters"
)]
use hypha_core::{
    Actor, AuthError, Ctx, Currency, ExchangeRate, IdempotencyRequest, LocationId, Money,
    PurchaseOrderId, ReceiptId, Scope, Timestamp, UnitCost, UserId, VariantId, Versioned,
};
use hypha_inventory::{
    CreateLocation, InventoryItem, InventoryItemRepo, StockReason, StockSource, create_location,
};
use hypha_purchasing::{
    CostAllocation, CostDraft, CostKind, LineInput, NewPurchaseOrder, Outstanding, PurchaseOrder,
    PurchaseOrderStatus, PurchasingError, ReceiptRepo, ReceiptState, ReceiptStatus, SupplierDraft,
    UpdatePurchaseOrder, VariantInfo, create_purchase_order, create_supplier, order_purchase_order,
};
use hypha_receiving::{
    CommandError, Receive, ReceiveLine, Received, cancel_receipt, close_purchase_order, receive,
    update_purchase_order,
};
use hypha_testkit::{FixedClock, InMemoryPorts, SequentialIds};
use pollster::block_on;
use uuid::Uuid;

fn vid(n: u128) -> VariantId {
    VariantId::from_uuid(Uuid::from_u128(n))
}

fn eur(minor: &str) -> UnitCost {
    UnitCost::parse(minor, Currency::EUR).unwrap()
}

fn line(variant: u128, accepted: u32, rejected: u32) -> ReceiveLine {
    ReceiveLine {
        variant: vid(variant),
        accepted,
        rejected,
    }
}

const NOW: i64 = 1_767_225_600;

struct World {
    ports: InMemoryPorts,
    ids: SequentialIds,
    clock: FixedClock,
    ctx: Ctx,
    location: LocationId,
}

impl World {
    fn new() -> Self {
        let ports = InMemoryPorts::default();
        let ids = SequentialIds::default();
        let location = block_on(create_location(
            &Ctx::system(),
            &ports.locations,
            &ids,
            CreateLocation {
                name: "Warehouse".to_string(),
                markets: Vec::new(),
                priority: 0,
            },
        ))
        .unwrap()
        .id();
        Self {
            ports,
            ids,
            clock: FixedClock::at(Timestamp::from_unix_micros(NOW * 1_000_000)),
            ctx: Ctx::system(),
            location,
        }
    }

    /// An ordered purchase order for `lines` (variant, quantity, unit cost in `currency`).
    fn ordered(
        &self,
        currency: Currency,
        rate: Option<&str>,
        lines: &[(u128, u32, &str)],
        costs: Vec<CostDraft>,
    ) -> PurchaseOrder {
        let supplier = block_on(create_supplier(
            &self.ctx,
            &self.ports,
            &self.ids,
            SupplierDraft::new("Spore Labs", currency),
        ))
        .unwrap()
        .id();
        let variants: Vec<VariantInfo> = lines
            .iter()
            .map(|&(v, ..)| VariantInfo {
                variant: vid(v),
                product_title: "Oyster grow kit".to_string(),
                variant_title: None,
                sku: None,
            })
            .collect();
        let po = block_on(create_purchase_order(
            &self.ctx,
            &self.ports,
            &self.ids,
            &variants,
            NewPurchaseOrder {
                destination: Some(self.location),
                exchange_rate: rate.map(|r| ExchangeRate::parse(r).unwrap()),
                lines: lines
                    .iter()
                    .map(|&(v, quantity, cost)| LineInput {
                        variant: vid(v),
                        quantity,
                        unit_cost: Some(UnitCost::parse(cost, currency).unwrap()),
                        supplier_sku: None,
                    })
                    .collect(),
                costs,
                ..NewPurchaseOrder::to(supplier)
            },
        ))
        .unwrap();
        block_on(order_purchase_order(
            &self.ctx,
            &self.ports,
            &self.clock,
            Some(Currency::EUR),
            po.id(),
        ))
        .unwrap()
    }

    fn receive_with(
        &self,
        po: PurchaseOrderId,
        lines: Vec<ReceiveLine>,
        allow_over: bool,
        key: Option<IdempotencyRequest>,
    ) -> Result<Received, CommandError> {
        block_on(receive(
            &self.ctx,
            &self.ports,
            &self.ids,
            &self.clock,
            Some(Currency::EUR),
            Receive {
                purchase_order: po,
                location: None,
                lines,
                allow_over,
                removed_variants: Vec::new(),
                note: None,
                idempotency: key,
            },
        ))
        .map(|r| r.value)
    }

    fn receive(
        &self,
        po: PurchaseOrderId,
        lines: Vec<ReceiveLine>,
    ) -> Result<Received, CommandError> {
        self.receive_with(po, lines, false, None)
    }

    fn item(&self, variant: u128) -> Option<InventoryItem> {
        block_on(self.ports.inventory_items.get_by_variant(vid(variant))).unwrap()
    }

    fn event_types(&self) -> Vec<String> {
        self.ports
            .outbox
            .events()
            .into_iter()
            .map(|e| e.event_type)
            .collect()
    }
}

fn tracked(w: &World, variant: u128, on_hand: i64, cost: Option<&str>) {
    let mut item = InventoryItem::new(
        hypha_core::InventoryItemId::from_uuid(Uuid::from_u128(9000 + variant)),
        vid(variant),
        true,
        false,
    );
    item.set_on_hand(w.location, on_hand, None, None).unwrap();
    item.set_unit_cost(cost.map(eur), None);
    block_on(w.ports.inventory_items.insert(&mut item)).unwrap();
}

#[test]
fn received_units_go_on_hand_at_their_landed_cost() {
    let w = World::new();
    tracked(&w, 10, 4, Some("500"));
    let po = w.ordered(
        Currency::EUR,
        None,
        &[(10, 6, "1000"), (11, 2, "300")],
        vec![],
    );
    let received = w.receive(po.id(), vec![line(10, 6, 0)]).unwrap();

    let item = w.item(10).unwrap();
    assert_eq!(item.on_hand(w.location), 10);
    // (4·500 + 6·1000) / 10
    assert_eq!(item.unit_cost(), Some(eur("800")));
    assert_eq!(
        received.receipt.lines()[0].landed_unit_cost,
        Some(eur("1000"))
    );
    assert_eq!(received.receipt.location(), w.location);
    assert_eq!(received.purchase_order.lines()[0].received(), 6);
    assert_eq!(
        received.purchase_order.receipt_status(),
        ReceiptStatus::Partial
    );
    assert_eq!(
        received.purchase_order.status(),
        PurchaseOrderStatus::Ordered
    );
    assert!(received.new_items.is_empty());

    let row = w
        .ports
        .outbox
        .movements()
        .into_iter()
        .rfind(|m| m.reason == StockReason::Received)
        .unwrap();
    assert_eq!(
        (row.delta, row.source, row.unit_cost, row.average_after),
        (
            6,
            StockSource::Receipt {
                purchase_order: po.id(),
                receipt: received.receipt.id()
            },
            Some(eur("1000")),
            Some(eur("800"))
        )
    );
    let stored = block_on(w.ports.receipts.get(received.receipt.id()))
        .unwrap()
        .unwrap();
    assert_eq!(stored.state(), ReceiptState::Recorded);
    let types = w.event_types();
    for expected in [
        "receipt.created",
        "purchase_order.receipt_status_changed",
        "inventory_item.cost_changed",
    ] {
        assert!(types.iter().any(|t| t == expected), "{expected}: {types:?}");
    }
}

#[test]
fn the_last_units_close_the_order() {
    let w = World::new();
    let po = w.ordered(Currency::EUR, None, &[(10, 3, "100")], vec![]);
    w.receive(po.id(), vec![line(10, 1, 0)]).unwrap();
    let received = w.receive(po.id(), vec![line(10, 2, 0)]).unwrap();
    assert_eq!(
        received.purchase_order.status(),
        PurchaseOrderStatus::Closed
    );
    assert_eq!(
        received.purchase_order.receipt_status(),
        ReceiptStatus::Received
    );
    assert!(w.event_types().iter().any(|t| t == "purchase_order.closed"));
}

#[test]
fn rejected_units_are_counted_but_add_no_stock() {
    let w = World::new();
    tracked(&w, 10, 0, None);
    let po = w.ordered(Currency::EUR, None, &[(10, 3, "100")], vec![]);
    let received = w.receive(po.id(), vec![line(10, 1, 2)]).unwrap();
    assert_eq!(w.item(10).unwrap().on_hand(w.location), 1);
    let counts = &received.purchase_order.lines()[0];
    assert_eq!(
        (counts.received(), counts.rejected(), counts.outstanding()),
        (1, 2, 0)
    );
    assert_eq!(
        received.purchase_order.status(),
        PurchaseOrderStatus::Closed
    );
    // Only rejected: nothing touches stock.
    let w = World::new();
    let po = w.ordered(Currency::EUR, None, &[(10, 3, "100")], vec![]);
    let received = w.receive(po.id(), vec![line(10, 0, 1)]).unwrap();
    assert!(received.new_items.is_empty());
    assert_eq!(w.item(10), None);
}

#[test]
fn more_than_is_outstanding_is_refused_unless_allowed() {
    let w = World::new();
    let po = w.ordered(
        Currency::EUR,
        None,
        &[(10, 3, "100"), (11, 1, "100")],
        vec![],
    );
    w.receive(po.id(), vec![line(10, 2, 0)]).unwrap();
    assert_eq!(
        w.receive(po.id(), vec![line(10, 1, 1), line(11, 2, 0)]),
        Err(CommandError::Purchasing(PurchasingError::OverReceived(
            vec![
                Outstanding {
                    variant: vid(10),
                    outstanding: 1
                },
                Outstanding {
                    variant: vid(11),
                    outstanding: 1
                },
            ]
        )))
    );
    assert_eq!(w.item(11), None, "nothing was written");
    let received = w
        .receive_with(po.id(), vec![line(10, 1, 1)], true, None)
        .unwrap();
    assert_eq!(received.purchase_order.lines()[0].received(), 3);
}

#[test]
fn a_variant_without_stock_gets_an_untracked_record() {
    let w = World::new();
    let po = w.ordered(Currency::EUR, None, &[(10, 5, "250")], vec![]);
    let received = w.receive(po.id(), vec![line(10, 5, 0)]).unwrap();
    assert_eq!(received.new_items, [vid(10)]);
    let item = w.item(10).unwrap();
    assert!(!item.tracked(), "receiving doesn't change whether it sells");
    assert_eq!(
        (item.on_hand(w.location), item.unit_cost()),
        (5, Some(eur("250")))
    );
}

#[test]
fn landed_cost_adds_the_extra_costs_at_the_orders_rate() {
    let w = World::new();
    // $10.00 and $5.00 a unit, 2 of each; $6.00 shipping by quantity is $1.50 a unit;
    // €4.00 duties paid at home, by value, is €1.3333 and €0.6667 a unit. At 0.9:
    // (1000 + 150) · 0.9 + 133.3333 = 1168.3333 cents, (500 + 150) · 0.9 + 66.6667 = 651.6667.
    let usd = |n: i64| Money::new(n, Currency::USD).unwrap();
    let po = w.ordered(
        Currency::USD,
        Some("0.9"),
        &[(10, 2, "1000"), (11, 2, "500")],
        vec![
            CostDraft {
                kind: CostKind::Shipping,
                description: None,
                amount: usd(600),
                allocation: CostAllocation::ByQuantity,
            },
            CostDraft {
                kind: CostKind::Duties,
                description: None,
                amount: Money::new(400, Currency::EUR).unwrap(),
                allocation: CostAllocation::ByValue,
            },
        ],
    );
    let received = w
        .receive(po.id(), vec![line(10, 2, 0), line(11, 1, 0)])
        .unwrap();
    let landed: Vec<_> = received
        .receipt
        .lines()
        .iter()
        .map(|l| l.landed_unit_cost)
        .collect();
    assert_eq!(landed, [Some(eur("1168.3333")), Some(eur("651.6667"))]);
    assert_eq!(
        received.receipt.exchange_rate(),
        Some(ExchangeRate::parse("0.9").unwrap())
    );
    assert_eq!(w.item(10).unwrap().unit_cost(), Some(eur("1168.3333")));
}

#[test]
fn a_receipt_is_refused_off_an_order_that_isnt_ordered() {
    let w = World::new();
    let po = w.ordered(Currency::EUR, None, &[(10, 1, "100")], vec![]);
    w.receive(po.id(), vec![line(10, 1, 0)]).unwrap();
    // Closed now.
    assert_eq!(
        w.receive(po.id(), vec![line(10, 1, 0)]),
        Err(CommandError::Purchasing(PurchasingError::NotOrdered {
            status: "closed"
        }))
    );
    let missing = PurchaseOrderId::from_uuid(Uuid::from_u128(77));
    assert_eq!(
        w.receive(missing, vec![line(10, 1, 0)]),
        Err(CommandError::PurchaseOrderNotFound(missing))
    );
}

#[test]
fn unknown_lines_and_places_are_refused() {
    let w = World::new();
    let po = w.ordered(Currency::EUR, None, &[(10, 2, "100")], vec![]);
    assert_eq!(
        w.receive(po.id(), vec![line(12, 1, 0)]),
        Err(CommandError::Purchasing(PurchasingError::UnknownLine(vid(
            12
        ))))
    );
    assert_eq!(
        w.receive(po.id(), vec![line(10, 0, 0)]),
        Err(CommandError::Purchasing(PurchasingError::EmptyReceiptLine(
            vid(10)
        )))
    );
    let elsewhere = LocationId::from_uuid(Uuid::from_u128(404));
    let refused = block_on(receive(
        &w.ctx,
        &w.ports,
        &w.ids,
        &w.clock,
        Some(Currency::EUR),
        Receive {
            purchase_order: po.id(),
            location: Some(elsewhere),
            lines: vec![line(10, 1, 0)],
            allow_over: false,
            removed_variants: Vec::new(),
            note: None,
            idempotency: None,
        },
    ));
    assert_eq!(refused, Err(CommandError::LocationNotFound(elsewhere)));
}

#[test]
fn receiving_needs_the_scope_and_a_base_currency() {
    let w = World::new();
    let po = w.ordered(Currency::EUR, None, &[(10, 2, "100")], vec![]);
    let input = Receive {
        purchase_order: po.id(),
        location: None,
        lines: vec![line(10, 1, 0)],
        allow_over: false,
        removed_variants: Vec::new(),
        note: None,
        idempotency: None,
    };
    let reader = Ctx::new(
        Actor::User(UserId::from_uuid(Uuid::from_u128(5))),
        vec![Scope::PurchasingRead],
    );
    assert_eq!(
        block_on(receive(
            &reader,
            &w.ports,
            &w.ids,
            &w.clock,
            Some(Currency::EUR),
            input.clone()
        )),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::PurchasingWrite
        )))
    );
    assert_eq!(
        block_on(receive(&w.ctx, &w.ports, &w.ids, &w.clock, None, input)),
        Err(CommandError::StoreCurrencyRequired)
    );
}

#[test]
fn the_same_key_answers_with_the_first_receipt() {
    let w = World::new();
    let po = w.ordered(Currency::EUR, None, &[(10, 4, "100")], vec![]);
    let key = || {
        Some(IdempotencyRequest {
            key: "rcv-1".to_string(),
            fingerprint: [7; 32],
        })
    };
    let first = w
        .receive_with(po.id(), vec![line(10, 2, 0)], false, key())
        .unwrap();
    let again = w
        .receive_with(po.id(), vec![line(10, 2, 0)], false, key())
        .unwrap();
    assert_eq!(again.receipt.id(), first.receipt.id());
    assert_eq!(w.item(10).unwrap().on_hand(w.location), 2, "received once");
    assert_eq!(again.purchase_order.lines()[0].received(), 2);
}

#[test]
fn taking_a_receipt_back_undoes_it() {
    let w = World::new();
    tracked(&w, 10, 4, Some("500"));
    // 4 at €5 and 6 at €10 average €8 exactly, so taking the 6 back lands on €5 again.
    let po = w.ordered(Currency::EUR, None, &[(10, 7, "1000")], vec![]);
    let received = w.receive(po.id(), vec![line(10, 6, 1)]).unwrap();
    assert_eq!(
        received.purchase_order.status(),
        PurchaseOrderStatus::Closed
    );

    let taken = block_on(cancel_receipt(
        &w.ctx,
        &w.ports,
        &w.clock,
        Some(Currency::EUR),
        received.receipt.id(),
    ))
    .unwrap();
    assert!(matches!(
        taken.receipt.state(),
        ReceiptState::Cancelled { .. }
    ));
    assert_eq!(taken.purchase_order.status(), PurchaseOrderStatus::Ordered);
    assert_eq!(taken.purchase_order.receipt_status(), ReceiptStatus::None);
    let counts = &taken.purchase_order.lines()[0];
    assert_eq!((counts.received(), counts.rejected()), (0, 0));
    let item = w.item(10).unwrap();
    assert_eq!(
        (item.on_hand(w.location), item.unit_cost()),
        (4, Some(eur("500")))
    );
    let row = w.ports.outbox.movements().pop().unwrap();
    assert_eq!((row.reason, row.delta), (StockReason::Received, -6));
    assert!(w.event_types().iter().any(|t| t == "receipt.cancelled"));

    assert_eq!(
        block_on(cancel_receipt(
            &w.ctx,
            &w.ports,
            &w.clock,
            Some(Currency::EUR),
            received.receipt.id()
        )),
        Err(CommandError::Purchasing(PurchasingError::ReceiptCancelled))
    );
    let missing = ReceiptId::from_uuid(Uuid::from_u128(66));
    assert_eq!(
        block_on(cancel_receipt(
            &w.ctx,
            &w.ports,
            &w.clock,
            Some(Currency::EUR),
            missing
        )),
        Err(CommandError::ReceiptNotFound(missing))
    );
}

// ---- late costs ----

fn shipping(amount: i64, currency: Currency) -> CostDraft {
    CostDraft {
        kind: CostKind::Shipping,
        description: Some("Carrier".to_string()),
        amount: Money::new(amount, currency).unwrap(),
        allocation: CostAllocation::ByQuantity,
    }
}

impl World {
    fn set_costs(
        &self,
        po: PurchaseOrderId,
        costs: Vec<CostDraft>,
    ) -> Result<PurchaseOrder, CommandError> {
        block_on(update_purchase_order(
            &self.ctx,
            &self.ports,
            &self.clock,
            Some(Currency::EUR),
            &[],
            po,
            UpdatePurchaseOrder {
                costs: Some(costs),
                ..UpdatePurchaseOrder::default()
            },
        ))
    }
}

#[test]
fn a_cost_added_after_goods_arrived_revalues_the_stock_on_hand() {
    let w = World::new();
    let po = w.ordered(Currency::EUR, None, &[(10, 10, "500")], vec![]);
    w.receive(po.id(), vec![line(10, 4, 0)]).unwrap();
    assert_eq!(w.item(10).unwrap().unit_cost(), Some(eur("500")));

    // €10.00 shipping over the 10 ordered: €1.00 a unit, €4.00 for the 4 that came,
    // spread over the 4 on hand.
    let po = w
        .set_costs(po.id(), vec![shipping(1000, Currency::EUR)])
        .unwrap();
    assert_eq!(po.lines()[0].extra_applied(), Some(4_000_000));
    let item = w.item(10).unwrap();
    assert_eq!(item.unit_cost(), Some(eur("600")));
    let row = w.ports.outbox.movements().pop().unwrap();
    assert_eq!(
        (row.reason, row.delta, row.location, row.source),
        (
            StockReason::CostAdjustment,
            0,
            None,
            StockSource::PurchaseOrder(po.id())
        )
    );
    // The rest arrives at the new landed cost: 6 at €6.00, the average stays €6.00.
    w.receive(po.id(), vec![line(10, 6, 0)]).unwrap();
    assert_eq!(w.item(10).unwrap().unit_cost(), Some(eur("600")));
    // Saving the same costs again changes nothing on the stock.
    let rows = w.ports.outbox.movements().len();
    w.set_costs(po.id(), vec![shipping(1000, Currency::EUR)])
        .unwrap();
    assert_eq!(w.ports.outbox.movements().len(), rows);
}

#[test]
fn a_cost_on_a_closed_order_still_revalues_and_only_costs_change() {
    let w = World::new();
    let po = w.ordered(Currency::EUR, None, &[(10, 4, "500")], vec![]);
    let received = w.receive(po.id(), vec![line(10, 4, 0)]).unwrap();
    assert_eq!(
        received.purchase_order.status(),
        PurchaseOrderStatus::Closed
    );
    let po = w
        .set_costs(po.id(), vec![shipping(800, Currency::EUR)])
        .unwrap();
    assert_eq!(po.status(), PurchaseOrderStatus::Closed);
    // €8.00 over 4: €2.00 more each.
    assert_eq!(w.item(10).unwrap().unit_cost(), Some(eur("700")));
    // Lowering it takes value back off.
    w.set_costs(po.id(), vec![shipping(400, Currency::EUR)])
        .unwrap();
    assert_eq!(w.item(10).unwrap().unit_cost(), Some(eur("600")));
    let refused = block_on(update_purchase_order(
        &w.ctx,
        &w.ports,
        &w.clock,
        Some(Currency::EUR),
        &[],
        po.id(),
        UpdatePurchaseOrder {
            note: Some(Some("too late".to_string())),
            ..UpdatePurchaseOrder::default()
        },
    ));
    assert_eq!(
        refused.map(|_| ()),
        Err(CommandError::PurchaseOrder(
            hypha_purchasing::CommandError::Purchasing(PurchasingError::NotOpen {
                status: "closed"
            })
        ))
    );
}

#[test]
fn units_already_sold_keep_their_cost() {
    let w = World::new();
    let po = w.ordered(Currency::EUR, None, &[(10, 10, "500")], vec![]);
    w.receive(po.id(), vec![line(10, 10, 0)]).unwrap();
    // 8 of the 10 are gone: the €10.00 the 10 carry lands on the 2 left (Odoo's rule:
    // the 8 sold aren't revalued after the fact).
    let mut item = w.item(10).unwrap();
    item.set_on_hand(w.location, 2, None, None).unwrap();
    block_on(w.ports.inventory_items.update(&mut item)).unwrap();
    w.set_costs(po.id(), vec![shipping(1000, Currency::EUR)])
        .unwrap();
    assert_eq!(w.item(10).unwrap().unit_cost(), Some(eur("1000")));
    // With nothing on hand nothing moves, but the order keeps what's due.
    let mut item = w.item(10).unwrap();
    item.set_on_hand(w.location, 0, None, None).unwrap();
    block_on(w.ports.inventory_items.update(&mut item)).unwrap();
    let version = w.item(10).unwrap().version();
    let po = w
        .set_costs(po.id(), vec![shipping(2000, Currency::EUR)])
        .unwrap();
    assert_eq!(w.item(10).unwrap().unit_cost(), Some(eur("1000")));
    assert_eq!(po.lines()[0].extra_applied(), Some(20_000_000));
    assert_eq!(
        w.item(10).unwrap().version(),
        version,
        "an item nothing happened to isn't written, so checkouts don't lose races to it"
    );
}

#[test]
fn units_over_the_order_carry_no_extra_cost() {
    let w = World::new();
    // €10.00 shipping over 10 ordered: €1.00 a unit.
    let po = w.ordered(
        Currency::EUR,
        None,
        &[(10, 10, "500")],
        vec![shipping(1000, Currency::EUR)],
    );
    // 12 arrive: the 10 ordered at €6.00, the 2 extra at €5.00, €70.00 in all.
    let received = w
        .receive_with(po.id(), vec![line(10, 12, 0)], true, None)
        .unwrap();
    assert_eq!(
        received.receipt.lines()[0].landed_unit_cost,
        Some(eur("583.3333"))
    );
    assert_eq!(w.item(10).unwrap().unit_cost(), Some(eur("583.3333")));
    // The order counts only the €10.00 paid, so a later change measures from it.
    assert_eq!(
        received.purchase_order.lines()[0].extra_applied(),
        Some(10_000_000)
    );
    let rows = w.ports.outbox.movements().len();
    let po = w
        .set_costs(po.id(), vec![shipping(1000, Currency::EUR)])
        .unwrap();
    assert_eq!(po.lines()[0].extra_applied(), Some(10_000_000));
    assert_eq!(w.ports.outbox.movements().len(), rows, "nothing to revalue");
}

#[test]
fn a_removed_variants_units_are_counted_but_go_into_no_stock() {
    let w = World::new();
    let po = w.ordered(
        Currency::EUR,
        None,
        &[(10, 3, "100"), (11, 2, "100")],
        vec![],
    );
    let received = block_on(receive(
        &w.ctx,
        &w.ports,
        &w.ids,
        &w.clock,
        Some(Currency::EUR),
        Receive {
            purchase_order: po.id(),
            location: None,
            lines: vec![line(10, 3, 0), line(11, 2, 0)],
            allow_over: false,
            removed_variants: vec![vid(10)],
            note: None,
            idempotency: None,
        },
    ))
    .unwrap()
    .value;
    assert_eq!(
        received.new_items,
        [vid(11)],
        "no record for the removed one"
    );
    assert_eq!(w.item(10), None);
    assert_eq!(received.purchase_order.lines()[0].received(), 3);
    assert_eq!(received.receipt.lines()[0].accepted, 3);
    assert_eq!(
        received.purchase_order.status(),
        PurchaseOrderStatus::Closed
    );
}

#[test]
fn a_late_cost_in_another_currency_converts_at_each_receipts_rate() {
    let w = World::new();
    let po = w.ordered(Currency::USD, Some("0.5"), &[(10, 4, "1000")], vec![]);
    w.receive(po.id(), vec![line(10, 2, 0)]).unwrap();
    // The rate changes before the second arrival.
    block_on(update_purchase_order(
        &w.ctx,
        &w.ports,
        &w.clock,
        Some(Currency::EUR),
        &[],
        po.id(),
        UpdatePurchaseOrder {
            exchange_rate: Some(Some(ExchangeRate::parse("1").unwrap())),
            ..UpdatePurchaseOrder::default()
        },
    ))
    .unwrap();
    w.receive(po.id(), vec![line(10, 2, 0)]).unwrap();
    // 2 at €5.00 and 2 at €10.00: €7.50.
    assert_eq!(w.item(10).unwrap().unit_cost(), Some(eur("750")));
    // $4.00 shipping is $1.00 a unit: €0.50 for the first two, €1.00 for the others,
    // €3.00 over 4 units, €0.75 more each.
    w.set_costs(po.id(), vec![shipping(400, Currency::USD)])
        .unwrap();
    assert_eq!(w.item(10).unwrap().unit_cost(), Some(eur("825")));
}

#[test]
fn closing_early_revalues_for_the_bigger_share() {
    let w = World::new();
    let po = w.ordered(
        Currency::EUR,
        None,
        &[(10, 10, "500")],
        vec![shipping(1000, Currency::EUR)],
    );
    // 5 arrive at €5.00 + €1.00 shipping.
    w.receive(po.id(), vec![line(10, 5, 0)]).unwrap();
    assert_eq!(w.item(10).unwrap().unit_cost(), Some(eur("600")));
    // Closed, the €10.00 is over the 5 that came: €2.00 each.
    let closed = block_on(close_purchase_order(
        &w.ctx,
        &w.ports,
        &w.clock,
        Some(Currency::EUR),
        po.id(),
    ))
    .unwrap();
    assert_eq!(closed.status(), PurchaseOrderStatus::Closed);
    assert_eq!(w.item(10).unwrap().unit_cost(), Some(eur("700")));
}

#[test]
fn taking_a_receipt_back_after_a_late_cost_leaves_the_rest_right() {
    let w = World::new();
    let po = w.ordered(Currency::EUR, None, &[(10, 10, "500")], vec![]);
    let first = w.receive(po.id(), vec![line(10, 4, 0)]).unwrap();
    w.receive(po.id(), vec![line(10, 4, 0)]).unwrap();
    // €10.00 shipping: €1.00 a unit, €8.00 over the 8 here.
    w.set_costs(po.id(), vec![shipping(1000, Currency::EUR)])
        .unwrap();
    assert_eq!(w.item(10).unwrap().unit_cost(), Some(eur("600")));
    // The first 4 leave at the €5.00 they came in at, and the €4.00 of shipping they
    // carried since comes off too: the 4 left are €6.00 each, as they should be.
    let taken = block_on(cancel_receipt(
        &w.ctx,
        &w.ports,
        &w.clock,
        Some(Currency::EUR),
        first.receipt.id(),
    ))
    .unwrap();
    let item = w.item(10).unwrap();
    assert_eq!(
        (item.on_hand(w.location), item.unit_cost()),
        (4, Some(eur("600")))
    );
    assert_eq!(
        taken.purchase_order.lines()[0].extra_applied(),
        Some(4_000_000)
    );
}

#[test]
fn an_order_nothing_arrived_on_changes_without_touching_stock() {
    let w = World::new();
    tracked(&w, 10, 3, Some("500"));
    let po = w.ordered(Currency::EUR, None, &[(10, 10, "500")], vec![]);
    let rows = w.ports.outbox.movements().len();
    let po = w
        .set_costs(po.id(), vec![shipping(1000, Currency::EUR)])
        .unwrap();
    assert_eq!(po.costs().len(), 1);
    assert_eq!(w.item(10).unwrap().unit_cost(), Some(eur("500")));
    assert_eq!(w.ports.outbox.movements().len(), rows);
}

#[test]
fn changing_an_order_needs_the_scope() {
    let w = World::new();
    let po = w.ordered(Currency::EUR, None, &[(10, 10, "500")], vec![]);
    let reader = Ctx::new(
        Actor::User(UserId::from_uuid(Uuid::from_u128(5))),
        vec![Scope::PurchasingRead],
    );
    let refused = block_on(update_purchase_order(
        &reader,
        &w.ports,
        &w.clock,
        Some(Currency::EUR),
        &[],
        po.id(),
        UpdatePurchaseOrder::default(),
    ));
    assert_eq!(
        refused.map(|_| ()),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::PurchasingWrite
        )))
    );
    let refused = block_on(close_purchase_order(
        &reader,
        &w.ports,
        &w.clock,
        Some(Currency::EUR),
        po.id(),
    ));
    assert_eq!(
        refused.map(|_| ()),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::PurchasingWrite
        )))
    );
}
