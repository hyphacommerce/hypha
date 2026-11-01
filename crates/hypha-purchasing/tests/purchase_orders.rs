//! Purchase orders over a store in memory.

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
    Actor, AuthError, Ctx, Currency, ExchangeRate, LocationId, PageLimit, PageRequest,
    PurchaseOrderId, Scope, SupplierId, Timestamp, UnitCost, UserId, VariantId,
};
use hypha_purchasing::{
    CommandError, LineInput, NewPurchaseOrder, PaymentDue, PaymentTermsDraft, PurchaseOrder,
    PurchaseOrderFilter, PurchaseOrderSort, PurchaseOrderStatus, PurchasingError, SupplierDraft,
    SupplierVariantDraft, UpdatePurchaseOrder, UpdateSupplier, VariantInfo, cancel_purchase_order,
    close_purchase_order, create_purchase_order, create_supplier, delete_purchase_order,
    delete_supplier, get_purchase_order, list_purchase_orders, order_purchase_order,
    set_supplier_variant, update_purchase_order, update_supplier,
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

fn info(n: u128) -> VariantInfo {
    VariantInfo {
        variant: vid(n),
        product_title: "Oyster grow kit".to_string(),
        variant_title: Some("Large".to_string()),
        sku: Some(format!("GK-{n}")),
    }
}

fn line(n: u128, quantity: u32, cost: Option<&str>) -> LineInput {
    LineInput {
        variant: vid(n),
        quantity,
        unit_cost: cost.map(eur),
        supplier_sku: None,
    }
}

const NOW: i64 = 1_767_225_600;

struct World {
    ports: InMemoryPorts,
    ids: SequentialIds,
    clock: FixedClock,
    ctx: Ctx,
    variants: Vec<VariantInfo>,
    store: Option<Currency>,
    location: LocationId,
}

impl World {
    fn new() -> Self {
        Self {
            ports: InMemoryPorts::default(),
            ids: SequentialIds::default(),
            clock: FixedClock::at(Timestamp::from_unix_micros(NOW * 1_000_000)),
            ctx: Ctx::system(),
            variants: (10..20).map(info).collect(),
            store: Some(Currency::EUR),
            location: LocationId::from_uuid(Uuid::from_u128(500)),
        }
    }

    fn supplier(&self) -> SupplierId {
        let mut draft = SupplierDraft::new("Spore Labs", Currency::EUR);
        draft.lead_time_days = Some(10);
        draft.payment_terms = Some(PaymentTermsDraft {
            due: PaymentDue::Net { days: 30 },
            note: None,
        });
        block_on(create_supplier(&self.ctx, &self.ports, &self.ids, draft))
            .unwrap()
            .id()
    }

    fn create(&self, input: NewPurchaseOrder) -> Result<PurchaseOrder, CommandError> {
        block_on(create_purchase_order(
            &self.ctx,
            &self.ports,
            &self.ids,
            &self.variants,
            input,
        ))
    }

    fn draft(&self, supplier: SupplierId, lines: Vec<LineInput>) -> PurchaseOrder {
        self.create(NewPurchaseOrder {
            destination: Some(self.location),
            lines,
            ..NewPurchaseOrder::to(supplier)
        })
        .unwrap()
    }

    fn update(
        &self,
        id: PurchaseOrderId,
        input: UpdatePurchaseOrder,
    ) -> Result<PurchaseOrder, CommandError> {
        block_on(update_purchase_order(
            &self.ctx,
            &self.ports,
            &self.clock,
            self.store,
            &self.variants,
            id,
            input,
        ))
    }

    fn order(&self, id: PurchaseOrderId) -> Result<PurchaseOrder, CommandError> {
        block_on(order_purchase_order(
            &self.ctx,
            &self.ports,
            &self.clock,
            self.store,
            id,
        ))
    }
}

fn missing<T>(scope: Scope) -> Result<T, CommandError> {
    Err(CommandError::Auth(AuthError::MissingScope(scope)))
}

#[test]
fn purchase_orders_need_purchasing_scopes() {
    let w = World::new();
    let supplier = w.supplier();
    let po = w.draft(supplier, vec![line(10, 1, Some("100"))]);
    let reader = Ctx::new(
        Actor::User(UserId::from_uuid(Uuid::from_u128(9))),
        vec![Scope::PurchasingRead],
    );
    assert_eq!(
        block_on(create_purchase_order(
            &reader,
            &w.ports,
            &w.ids,
            &w.variants,
            NewPurchaseOrder::to(supplier)
        )),
        missing(Scope::PurchasingWrite)
    );
    assert_eq!(
        block_on(order_purchase_order(
            &reader,
            &w.ports,
            &w.clock,
            w.store,
            po.id()
        )),
        missing(Scope::PurchasingWrite)
    );
    assert!(block_on(get_purchase_order(&reader, &w.ports, po.id())).is_ok());
    let nobody = Ctx::new(Actor::User(UserId::from_uuid(Uuid::from_u128(9))), vec![]);
    assert_eq!(
        block_on(get_purchase_order(&nobody, &w.ports, po.id())),
        missing(Scope::PurchasingRead)
    );
}

#[test]
fn a_draft_takes_the_suppliers_currency_and_terms_and_numbers_from_1001() {
    let w = World::new();
    let supplier = w.supplier();
    let first = w.draft(supplier, vec![]);
    let second = w.draft(supplier, vec![]);
    assert_eq!(first.number().to_string(), "PO-1001");
    assert_eq!(second.number().to_string(), "PO-1002");
    assert_eq!(first.status(), PurchaseOrderStatus::Draft);
    assert_eq!(first.currency(), Currency::EUR);
    assert_eq!(
        first.payment_terms().map(|t| t.due()),
        Some(PaymentDue::Net { days: 30 })
    );
    let usd = w
        .create(NewPurchaseOrder {
            currency: Some(Currency::USD),
            payment_terms: Some(None),
            ..NewPurchaseOrder::to(supplier)
        })
        .unwrap();
    assert_eq!(usd.currency(), Currency::USD);
    assert_eq!(usd.payment_terms(), None);
}

#[test]
fn a_line_starts_from_the_catalogue_then_the_last_order() {
    let w = World::new();
    let supplier = w.supplier();
    block_on(set_supplier_variant(
        &w.ctx,
        &w.ports,
        supplier,
        vid(10),
        SupplierVariantDraft {
            supplier_sku: Some("SL-10".to_string()),
            unit_cost: Some(eur("350")),
            ..SupplierVariantDraft::default()
        },
    ))
    .unwrap();
    let first = w.draft(supplier, vec![line(10, 2, None), line(11, 1, Some("99"))]);
    let l10 = first.line(vid(10)).unwrap();
    assert_eq!(l10.unit_cost(), eur("350"));
    assert_eq!(l10.supplier_sku(), Some("SL-10"));
    assert_eq!(l10.product_title(), "Oyster grow kit");
    assert_eq!(l10.sku(), Some("GK-10"));

    // Variant 11 isn't in the catalogue: the last order's line has its cost.
    let second = w.draft(supplier, vec![line(11, 4, None)]);
    assert_eq!(second.line(vid(11)).unwrap().unit_cost(), eur("99"));

    // Neither knows variant 12.
    assert_eq!(
        w.create(NewPurchaseOrder {
            lines: vec![line(12, 1, None)],
            ..NewPurchaseOrder::to(supplier)
        }),
        Err(CommandError::UnitCostRequired(vid(12)))
    );
    // A cost in another currency isn't taken for a line in this one.
    assert_eq!(
        w.create(NewPurchaseOrder {
            currency: Some(Currency::USD),
            lines: vec![line(10, 1, None)],
            ..NewPurchaseOrder::to(supplier)
        }),
        Err(CommandError::UnitCostRequired(vid(10)))
    );
}

#[test]
fn a_line_for_a_variant_the_store_lacks_is_refused() {
    let w = World::new();
    let supplier = w.supplier();
    assert_eq!(
        w.create(NewPurchaseOrder {
            lines: vec![line(99, 1, Some("1"))],
            ..NewPurchaseOrder::to(supplier)
        }),
        Err(CommandError::UnknownVariant(vid(99)))
    );
}

#[test]
fn archived_suppliers_take_no_new_orders() {
    let w = World::new();
    let supplier = w.supplier();
    let po = w.draft(supplier, vec![line(10, 1, Some("1"))]);
    block_on(update_supplier(
        &w.ctx,
        &w.ports,
        supplier,
        UpdateSupplier {
            archived: Some(true),
            ..UpdateSupplier::default()
        },
    ))
    .unwrap();
    assert_eq!(
        w.create(NewPurchaseOrder::to(supplier)),
        Err(CommandError::SupplierArchived(supplier))
    );
    assert_eq!(
        w.order(po.id()),
        Err(CommandError::SupplierArchived(supplier))
    );
}

#[test]
fn an_unknown_supplier_or_order_is_not_found() {
    let w = World::new();
    let ghost = SupplierId::from_uuid(Uuid::from_u128(77));
    assert_eq!(
        w.create(NewPurchaseOrder::to(ghost)),
        Err(CommandError::SupplierNotFound(ghost))
    );
    let po = PurchaseOrderId::from_uuid(Uuid::from_u128(78));
    assert_eq!(
        block_on(get_purchase_order(&w.ctx, &w.ports, po)),
        Err(CommandError::PurchaseOrderNotFound(po))
    );
}

#[test]
fn ordering_keeps_the_supplier_and_sets_the_expected_date() {
    let w = World::new();
    let supplier = w.supplier();
    let po = w.draft(supplier, vec![line(10, 2, Some("100"))]);
    let ordered = w.order(po.id()).unwrap();
    assert_eq!(ordered.status(), PurchaseOrderStatus::Ordered);
    assert_eq!(ordered.supplier_name(), Some("Spore Labs"));
    assert_eq!(
        ordered.expected_at(),
        Some(Timestamp::from_unix_micros((NOW + 10 * 86_400) * 1_000_000))
    );
    assert_eq!(
        block_on(get_purchase_order(&w.ctx, &w.ports, po.id())),
        Ok(ordered)
    );
}

#[test]
fn ordering_needs_the_stores_currency_and_a_rate_for_another() {
    let mut w = World::new();
    let supplier = w.supplier();
    let po = w.draft(supplier, vec![line(10, 2, Some("100"))]);
    w.store = None;
    assert_eq!(
        w.order(po.id()),
        Err(PurchasingError::StoreCurrencyRequired.into())
    );
    w.store = Some(Currency::USD);
    assert_eq!(
        w.order(po.id()),
        Err(PurchasingError::ExchangeRateRequired {
            currency: Currency::EUR,
            store: Currency::USD
        }
        .into())
    );
    w.update(
        po.id(),
        UpdatePurchaseOrder {
            exchange_rate: Some(Some(ExchangeRate::parse("1.08").unwrap())),
            ..UpdatePurchaseOrder::default()
        },
    )
    .unwrap();
    assert!(w.order(po.id()).is_ok());
}

#[test]
fn editing_an_ordered_order_counts_revisions_and_keeps_existing_costs() {
    let w = World::new();
    let supplier = w.supplier();
    let po = w.draft(supplier, vec![line(10, 2, Some("100"))]);
    w.order(po.id()).unwrap();
    let edited = w
        .update(
            po.id(),
            UpdatePurchaseOrder {
                lines: Some(vec![line(10, 5, None), line(11, 1, Some("7"))]),
                note: Some(Some("Call before delivering".to_string())),
                ..UpdatePurchaseOrder::default()
            },
        )
        .unwrap();
    assert_eq!(edited.revision(), 1);
    assert_eq!(edited.line(vid(10)).unwrap().quantity(), 5);
    assert_eq!(edited.line(vid(10)).unwrap().unit_cost(), eur("100"));
    assert_eq!(edited.note(), Some("Call before delivering"));
}

#[test]
fn changing_the_currency_needs_lines_in_it() {
    let w = World::new();
    let supplier = w.supplier();
    let po = w.draft(supplier, vec![line(10, 2, Some("100"))]);
    assert_eq!(
        w.update(
            po.id(),
            UpdatePurchaseOrder {
                currency: Some(Currency::USD),
                ..UpdatePurchaseOrder::default()
            },
        ),
        Err(PurchasingError::LineCurrencyMismatch {
            variant: vid(10),
            expected: Currency::USD,
            got: Currency::EUR,
        }
        .into())
    );
    let usd = w
        .update(
            po.id(),
            UpdatePurchaseOrder {
                currency: Some(Currency::USD),
                lines: Some(vec![LineInput {
                    unit_cost: Some(UnitCost::parse("110", Currency::USD).unwrap()),
                    ..line(10, 2, None)
                }]),
                ..UpdatePurchaseOrder::default()
            },
        )
        .unwrap();
    assert_eq!(usd.currency(), Currency::USD);
}

#[test]
fn closing_cancelling_and_deleting_follow_the_status() {
    let w = World::new();
    let supplier = w.supplier();
    let draft = w.draft(supplier, vec![line(10, 2, Some("100"))]);
    assert_eq!(
        block_on(close_purchase_order(&w.ctx, &w.ports, &w.clock, draft.id())),
        Err(PurchasingError::NotOrdered { status: "draft" }.into())
    );
    w.order(draft.id()).unwrap();
    assert_eq!(
        block_on(delete_purchase_order(&w.ctx, &w.ports, draft.id())),
        Err(PurchasingError::NotDraft { status: "ordered" }.into())
    );
    let closed = block_on(close_purchase_order(&w.ctx, &w.ports, &w.clock, draft.id())).unwrap();
    assert_eq!(closed.status(), PurchaseOrderStatus::Closed);
    assert_eq!(closed.lines()[0].cancelled(), 2);

    let other = w.draft(supplier, vec![line(10, 2, Some("100"))]);
    let cancelled = block_on(cancel_purchase_order(
        &w.ctx,
        &w.ports,
        &w.clock,
        other.id(),
    ))
    .unwrap();
    assert_eq!(cancelled.status(), PurchaseOrderStatus::Cancelled);

    let doomed = w.draft(supplier, vec![]);
    block_on(delete_purchase_order(&w.ctx, &w.ports, doomed.id())).unwrap();
    assert_eq!(
        block_on(get_purchase_order(&w.ctx, &w.ports, doomed.id())),
        Err(CommandError::PurchaseOrderNotFound(doomed.id()))
    );
}

#[test]
fn a_supplier_on_an_order_is_archived_not_deleted() {
    let w = World::new();
    let supplier = w.supplier();
    let po = w.draft(supplier, vec![]);
    assert_eq!(
        block_on(delete_supplier(&w.ctx, &w.ports, supplier)),
        Err(CommandError::SupplierInUse(supplier))
    );
    block_on(delete_purchase_order(&w.ctx, &w.ports, po.id())).unwrap();
    assert_eq!(
        block_on(delete_supplier(&w.ctx, &w.ports, supplier)),
        Ok(())
    );
}

#[test]
fn lists_filter_and_sort() {
    let w = World::new();
    let supplier = w.supplier();
    let a = w.draft(supplier, vec![line(10, 1, Some("1"))]);
    let b = w.draft(supplier, vec![line(11, 1, Some("1"))]);
    w.order(b.id()).unwrap();
    let list = |filter: PurchaseOrderFilter, sort: PurchaseOrderSort| -> Vec<PurchaseOrderId> {
        block_on(list_purchase_orders(
            &w.ctx,
            &w.ports,
            filter,
            sort,
            PageRequest {
                after: None,
                limit: PageLimit::new(10).unwrap(),
            },
        ))
        .unwrap()
        .items
        .iter()
        .map(|s| s.id)
        .collect()
    };
    assert_eq!(
        list(
            PurchaseOrderFilter::default(),
            PurchaseOrderSort::NumberDesc
        ),
        [b.id(), a.id()]
    );
    let ordered = PurchaseOrderFilter {
        status: Some(PurchaseOrderStatus::Ordered),
        ..PurchaseOrderFilter::default()
    };
    assert_eq!(list(ordered, PurchaseOrderSort::NumberAsc), [b.id()]);
}
