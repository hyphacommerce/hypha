//! Suppliers and their catalogues over a store in memory.

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
    Actor, AuthError, Ctx, Currency, PageLimit, PageRequest, Scope, SupplierId, UnitCost, UserId,
    VariantId, Versioned,
};
use hypha_purchasing::{
    CommandError, ContactDraft, PaymentDue, PaymentTermsDraft, PurchasingError, SupplierDraft,
    SupplierFilter, SupplierVariantDraft, UpdateSupplier, create_supplier, delete_supplier,
    get_supplier, list_supplier_variants, list_suppliers, remove_supplier_variant,
    set_supplier_variant, supplier_variant, update_supplier, variant_suppliers,
};
use hypha_testkit::{InMemoryPorts, SequentialIds};
use pollster::block_on;
use uuid::Uuid;

fn vid(n: u128) -> VariantId {
    VariantId::from_uuid(Uuid::from_u128(n))
}

fn page<Id>(after: Option<Id>) -> PageRequest<Id> {
    PageRequest {
        limit: PageLimit::new(10).unwrap(),
        after,
    }
}

fn eur(minor: &str) -> UnitCost {
    UnitCost::parse(minor, Currency::EUR).unwrap()
}

struct World {
    ports: InMemoryPorts,
    ids: SequentialIds,
    ctx: Ctx,
}

impl World {
    fn new() -> Self {
        Self {
            ports: InMemoryPorts::default(),
            ids: SequentialIds::default(),
            ctx: Ctx::system(),
        }
    }

    fn supplier(&self, name: &str) -> SupplierId {
        block_on(create_supplier(
            &self.ctx,
            &self.ports,
            &self.ids,
            SupplierDraft::new(name, Currency::EUR),
        ))
        .unwrap()
        .id()
    }

    fn set(&self, supplier: SupplierId, variant: u128, cost: Option<UnitCost>) {
        block_on(set_supplier_variant(
            &self.ctx,
            &self.ports,
            supplier,
            vid(variant),
            SupplierVariantDraft {
                unit_cost: cost,
                ..SupplierVariantDraft::default()
            },
        ))
        .unwrap();
    }

    fn update(&self, id: SupplierId, input: UpdateSupplier) -> Result<u64, CommandError> {
        block_on(update_supplier(&self.ctx, &self.ports, id, input)).map(|s| s.version().value())
    }
}

fn missing<T>(scope: Scope) -> Result<T, CommandError> {
    Err(CommandError::Auth(AuthError::MissingScope(scope)))
}

#[test]
fn reading_and_writing_need_purchasing_scopes() {
    let w = World::new();
    let id = w.supplier("Spore Labs");
    let staff = |scopes| Ctx::new(Actor::User(UserId::from_uuid(Uuid::from_u128(5))), scopes);
    let reader = staff(vec![Scope::PurchasingRead]);
    assert!(block_on(get_supplier(&reader, &w.ports, id)).is_ok());
    assert_eq!(
        block_on(create_supplier(
            &reader,
            &w.ports,
            &w.ids,
            SupplierDraft::new("Other", Currency::EUR)
        ))
        .map(|s| s.id()),
        missing(Scope::PurchasingWrite)
    );
    assert_eq!(
        block_on(get_supplier(&staff(vec![]), &w.ports, id)).map(|s| s.id()),
        missing(Scope::PurchasingRead)
    );
    assert_eq!(
        block_on(variant_suppliers(&staff(vec![]), &w.ports, vid(1))).map(|rows| rows.len()),
        missing(Scope::PurchasingRead)
    );
    // Writing implies reading.
    let writer = staff(vec![Scope::PurchasingWrite]);
    assert!(block_on(get_supplier(&writer, &w.ports, id)).is_ok());
}

#[test]
fn a_supplier_is_created_with_every_detail() {
    let w = World::new();
    let mut draft = SupplierDraft::new("Spore Labs", Currency::EUR);
    draft.contacts = vec![ContactDraft {
        name: "Ana".to_string(),
        email: Some("ana@sporelabs.example".to_string()),
        phone: None,
        primary: true,
    }];
    draft.payment_terms = Some(PaymentTermsDraft {
        due: PaymentDue::Net { days: 30 },
        note: None,
    });
    draft.lead_time_days = Some(14);
    let created = block_on(create_supplier(&w.ctx, &w.ports, &w.ids, draft)).unwrap();
    let read = block_on(get_supplier(&w.ctx, &w.ports, created.id())).unwrap();
    assert_eq!(read, created);
    assert_eq!(
        read.primary_contact()
            .and_then(|c| c.email())
            .map(|e| e.as_str()),
        Some("ana@sporelabs.example")
    );
    assert_eq!(
        read.payment_terms().map(|t| t.due()),
        Some(PaymentDue::Net { days: 30 })
    );
}

#[test]
fn an_unknown_supplier_is_not_found() {
    let w = World::new();
    let id = SupplierId::from_uuid(Uuid::from_u128(99));
    assert_eq!(
        block_on(get_supplier(&w.ctx, &w.ports, id)),
        Err(CommandError::SupplierNotFound(id))
    );
    assert_eq!(
        w.update(id, UpdateSupplier::default()),
        Err(CommandError::SupplierNotFound(id))
    );
    assert_eq!(
        block_on(delete_supplier(&w.ctx, &w.ports, id)),
        Err(CommandError::SupplierNotFound(id))
    );
}

#[test]
fn update_changes_only_what_it_names_and_writes_nothing_otherwise() {
    let w = World::new();
    let id = w.supplier("Spore Labs");
    assert_eq!(w.update(id, UpdateSupplier::default()), Ok(1));
    assert_eq!(
        w.update(
            id,
            UpdateSupplier {
                notes: Some(Some("Ships Mondays".to_string())),
                lead_time_days: Some(Some(10)),
                ..UpdateSupplier::default()
            }
        ),
        Ok(2)
    );
    let read = block_on(get_supplier(&w.ctx, &w.ports, id)).unwrap();
    assert_eq!(read.name(), "Spore Labs");
    assert_eq!(read.notes(), Some("Ships Mondays"));
    assert_eq!(
        w.update(
            id,
            UpdateSupplier {
                notes: Some(None),
                archived: Some(true),
                ..UpdateSupplier::default()
            }
        ),
        Ok(3)
    );
    let read = block_on(get_supplier(&w.ctx, &w.ports, id)).unwrap();
    assert_eq!(
        (read.notes(), read.archived(), read.lead_time_days()),
        (None, true, Some(10))
    );
}

#[test]
fn a_bad_update_changes_nothing() {
    let w = World::new();
    let id = w.supplier("Spore Labs");
    assert_eq!(
        w.update(
            id,
            UpdateSupplier {
                name: Some(" ".to_string()),
                archived: Some(true),
                ..UpdateSupplier::default()
            }
        ),
        Err(CommandError::Purchasing(PurchasingError::Text(
            hypha_core::TextError::Missing("name")
        )))
    );
    assert!(
        !block_on(get_supplier(&w.ctx, &w.ports, id))
            .unwrap()
            .archived()
    );
}

#[test]
fn the_currency_stays_while_the_catalogue_has_costs() {
    let w = World::new();
    let id = w.supplier("Spore Labs");
    w.set(id, 1, None);
    let to_usd = || UpdateSupplier {
        currency: Some(Currency::USD),
        ..UpdateSupplier::default()
    };
    // Rows without a cost don't hold it.
    assert_eq!(w.update(id, to_usd()), Ok(2));
    w.update(
        id,
        UpdateSupplier {
            currency: Some(Currency::EUR),
            ..UpdateSupplier::default()
        },
    )
    .unwrap();
    w.set(id, 2, Some(eur("350")));
    assert_eq!(
        w.update(id, to_usd()),
        Err(CommandError::SupplierCurrencyInUse(id))
    );
    // Its other fields still change.
    assert_eq!(
        w.update(
            id,
            UpdateSupplier {
                currency: Some(Currency::EUR),
                name: Some("Spore Labs Ltd".to_string()),
                ..UpdateSupplier::default()
            }
        ),
        Ok(4)
    );
}

#[test]
fn deleting_a_supplier_takes_its_catalogue() {
    let w = World::new();
    let id = w.supplier("Spore Labs");
    let other = w.supplier("Other");
    w.set(id, 1, Some(eur("350")));
    w.set(other, 1, None);
    block_on(delete_supplier(&w.ctx, &w.ports, id)).unwrap();
    assert_eq!(
        block_on(get_supplier(&w.ctx, &w.ports, id)),
        Err(CommandError::SupplierNotFound(id))
    );
    let sellers = block_on(variant_suppliers(&w.ctx, &w.ports, vid(1))).unwrap();
    assert_eq!(sellers.len(), 1);
    assert_eq!(sellers[0].supplier(), other);
}

#[test]
fn lists_filter_by_archived_and_name() {
    let w = World::new();
    let labs = w.supplier("Spore Labs");
    let glass = w.supplier("Lab Glass");
    w.update(
        glass,
        UpdateSupplier {
            archived: Some(true),
            ..UpdateSupplier::default()
        },
    )
    .unwrap();
    let list = |filter| {
        block_on(list_suppliers(&w.ctx, &w.ports, filter, page(None)))
            .unwrap()
            .items
            .iter()
            .map(|s| s.id())
            .collect::<Vec<_>>()
    };
    assert_eq!(list(SupplierFilter::default()), [glass, labs]);
    assert_eq!(
        list(SupplierFilter {
            archived: Some(false),
            q: None
        }),
        [labs]
    );
    assert_eq!(
        list(SupplierFilter {
            archived: None,
            q: Some("glass".to_string())
        }),
        [glass]
    );
}

#[test]
fn catalogue_rows_are_set_replaced_read_and_removed() {
    let w = World::new();
    let id = w.supplier("Spore Labs");
    let row = block_on(set_supplier_variant(
        &w.ctx,
        &w.ports,
        id,
        vid(1),
        SupplierVariantDraft {
            supplier_sku: Some("SL-KIT".to_string()),
            unit_cost: Some(eur("0.35")),
            min_order_quantity: Some(100),
            lead_time_days: Some(5),
        },
    ))
    .unwrap();
    assert_eq!(row.supplier_sku(), Some("SL-KIT"));
    assert_eq!(
        block_on(supplier_variant(&w.ctx, &w.ports, id, vid(1))),
        Ok(row)
    );
    w.set(id, 1, Some(eur("400")));
    let replaced = block_on(supplier_variant(&w.ctx, &w.ports, id, vid(1))).unwrap();
    assert_eq!(
        (replaced.supplier_sku(), replaced.unit_cost()),
        (None, Some(eur("400")))
    );
    w.set(id, 2, None);
    let listed = block_on(list_supplier_variants(&w.ctx, &w.ports, id, page(None))).unwrap();
    assert_eq!(listed.items.len(), 2);

    block_on(remove_supplier_variant(&w.ctx, &w.ports, id, vid(1))).unwrap();
    assert_eq!(
        block_on(remove_supplier_variant(&w.ctx, &w.ports, id, vid(1))),
        Err(CommandError::SupplierVariantNotFound {
            supplier: id,
            variant: vid(1)
        })
    );
    assert_eq!(
        block_on(supplier_variant(&w.ctx, &w.ports, id, vid(1))),
        Err(CommandError::SupplierVariantNotFound {
            supplier: id,
            variant: vid(1)
        })
    );
}

#[test]
fn a_catalogue_cost_is_in_the_suppliers_currency() {
    let w = World::new();
    let id = w.supplier("Spore Labs");
    assert_eq!(
        block_on(set_supplier_variant(
            &w.ctx,
            &w.ports,
            id,
            vid(1),
            SupplierVariantDraft {
                unit_cost: Some(UnitCost::parse("350", Currency::USD).unwrap()),
                ..SupplierVariantDraft::default()
            },
        )),
        Err(CommandError::Purchasing(
            PurchasingError::CostCurrencyMismatch {
                expected: Currency::EUR,
                got: Currency::USD
            }
        ))
    );
}

#[test]
fn a_catalogue_needs_its_supplier() {
    let w = World::new();
    let id = SupplierId::from_uuid(Uuid::from_u128(99));
    assert_eq!(
        block_on(set_supplier_variant(
            &w.ctx,
            &w.ports,
            id,
            vid(1),
            SupplierVariantDraft::default()
        )),
        Err(CommandError::SupplierNotFound(id))
    );
    assert_eq!(
        block_on(list_supplier_variants(&w.ctx, &w.ports, id, page(None))),
        Err(CommandError::SupplierNotFound(id))
    );
}
