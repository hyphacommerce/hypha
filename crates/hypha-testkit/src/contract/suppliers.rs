//! What every purchasing store must do: suppliers with every field, and their catalogues.

use hypha_core::{
    AddressParts, Currency, HasOutbox, OutboxRepo, PageLimit, PageRequest, RepoError, Stamped,
    SupplierId, UnitCost, VariantId, Version, Versioned,
};
use hypha_purchasing::{
    ContactDraft, HasSupplierVariants, HasSuppliers, PaymentDue, PaymentTermsDraft, Supplier,
    SupplierDraft, SupplierFilter, SupplierRepo, SupplierVariant, SupplierVariantDraft,
    SupplierVariantRepo,
};
use uuid::Uuid;

use super::scenario;

fn sid(n: u128) -> SupplierId {
    SupplierId::from_uuid(Uuid::from_u128(n))
}

fn vid(n: u128) -> VariantId {
    VariantId::from_uuid(Uuid::from_u128(n))
}

fn supplier(n: u128, name: &str) -> Supplier {
    Supplier::new(sid(n), SupplierDraft::new(name, Currency::EUR)).unwrap()
}

fn page<Id>(limit: u32, after: Option<Id>) -> PageRequest<Id> {
    PageRequest {
        limit: PageLimit::new(limit).unwrap(),
        after,
    }
}

fn row(supplier: u128, variant: u128, cost: Option<&str>) -> SupplierVariant {
    SupplierVariant::new(
        sid(supplier),
        Currency::EUR,
        vid(variant),
        SupplierVariantDraft {
            supplier_sku: Some(format!("SL-{variant}")),
            unit_cost: cost.map(|c| UnitCost::parse(c, Currency::EUR).unwrap()),
            min_order_quantity: Some(12),
            lead_time_days: Some(7),
        },
    )
    .unwrap()
}

/// Runs every supplier scenario, each on fresh ports from `make`.
pub async fn supplier_repo<S>(make: impl AsyncFn() -> S)
where
    S: HasSuppliers + HasSupplierVariants + HasOutbox,
{
    scenario("supplier: every field survives a round trip, at version 1, stamped");
    round_trip(&make().await).await;
    scenario("supplier: update bumps the version and writes its event; a stale copy fails");
    updates(&make().await).await;
    scenario("supplier: delete takes its catalogue and writes its event; a stale copy fails");
    deletes(&make().await).await;
    scenario("supplier: lists filter by archived and by name ignoring case, newest first");
    lists(&make().await).await;
}

/// Runs every catalogue scenario, each on fresh ports from `make`.
pub async fn supplier_variants<S>(make: impl AsyncFn() -> S)
where
    S: HasSuppliers + HasSupplierVariants,
{
    scenario("supplier catalogue: a row round-trips and is replaced by the next put");
    put_and_get(&make().await).await;
    scenario("supplier catalogue: a row for a missing supplier is refused");
    missing_supplier(&make().await).await;
    scenario("supplier catalogue: rows page by variant, newest first, per supplier");
    catalogue_pages(&make().await).await;
    scenario("supplier catalogue: a variant's rows across suppliers, by supplier");
    per_variant(&make().await).await;
    scenario("supplier catalogue: rows are removed one by one and by variant");
    removing(&make().await).await;
    scenario("supplier catalogue: costs are found per supplier");
    costs(&make().await).await;
}

fn full_draft() -> SupplierDraft {
    SupplierDraft {
        name: "Spore Labs".to_string(),
        contacts: vec![
            ContactDraft {
                name: "Ana Kovač".to_string(),
                email: Some("ana@sporelabs.example".to_string()),
                phone: Some("+385 1 234 5678".to_string()),
                primary: true,
            },
            ContactDraft {
                name: "Accounts".to_string(),
                email: Some("accounts@sporelabs.example".to_string()),
                phone: None,
                primary: false,
            },
        ],
        address: Some(AddressParts {
            first_name: None,
            last_name: "Receiving".to_string(),
            company: Some("Spore Labs d.o.o.".to_string()),
            line1: "Ilica 1".to_string(),
            line2: None,
            city: "Zagreb".to_string(),
            province: None,
            postal_code: Some("10000".to_string()),
            country: "HR".to_string(),
            phone: None,
        }),
        currency: Currency::EUR,
        payment_terms: Some(PaymentTermsDraft {
            due: PaymentDue::Net { days: 30 },
            note: Some("2% off within 10 days".to_string()),
        }),
        lead_time_days: Some(14),
        tax_number: Some("HR12345678901".to_string()),
        notes: Some("Ships Mondays.".to_string()),
        archived: false,
    }
}

async fn round_trip<S: HasSuppliers>(ports: &S) {
    let repo = ports.suppliers();
    let mut full = Supplier::new(sid(1), full_draft()).unwrap();
    repo.insert(&mut full).await.unwrap();
    assert_eq!(full.version(), Version::new(1));
    let stored = repo.get(sid(1)).await.unwrap().unwrap();
    assert_eq!(stored, full);
    assert!(stored.stamps().is_some());

    // Each payment term kind, and nothing optional at all.
    for (n, due) in [(2, PaymentDue::InAdvance), (3, PaymentDue::OnReceipt)] {
        let mut draft = SupplierDraft::new("Plain", Currency::USD);
        draft.payment_terms = Some(PaymentTermsDraft { due, note: None });
        let mut plain = Supplier::new(sid(n), draft).unwrap();
        repo.insert(&mut plain).await.unwrap();
        assert_eq!(repo.get(sid(n)).await, Ok(Some(plain)));
    }
    let mut bare = supplier(4, "Bare");
    repo.insert(&mut bare).await.unwrap();
    assert_eq!(repo.get(sid(4)).await, Ok(Some(bare)));
    assert_eq!(repo.get(sid(9)).await, Ok(None));
    assert_eq!(
        repo.insert(&mut supplier(1, "Again")).await,
        Err(RepoError::Duplicate(hypha_purchasing::keys::SUPPLIER_ID))
    );
}

async fn updates<S: HasSuppliers + HasOutbox>(ports: &S) {
    let repo = ports.suppliers();
    let mut s = supplier(1, "Spore Labs");
    repo.insert(&mut s).await.unwrap();
    let stale = s.clone();
    let mut draft = full_draft();
    draft.archived = true;
    s.update(draft).unwrap();
    repo.update(&mut s).await.unwrap();
    assert_eq!(s.version(), Version::new(2));
    assert_eq!(repo.get(sid(1)).await, Ok(Some(s.clone())));

    // Clearing every optional field round-trips too.
    s.update(SupplierDraft::new("Spore Labs", Currency::EUR))
        .unwrap();
    repo.update(&mut s).await.unwrap();
    assert_eq!(repo.get(sid(1)).await, Ok(Some(s)));

    let mut stale = stale;
    stale.update(full_draft()).unwrap();
    assert_eq!(repo.update(&mut stale).await, Err(RepoError::StaleVersion));

    let types: Vec<String> = ports
        .outbox()
        .events_after(0, 100)
        .await
        .unwrap()
        .into_iter()
        .map(|e| e.event_type)
        .collect();
    assert_eq!(
        types,
        ["supplier.created", "supplier.updated", "supplier.updated"]
    );
}

async fn deletes<S: HasSuppliers + HasSupplierVariants + HasOutbox>(ports: &S) {
    let repo = ports.suppliers();
    let mut s = supplier(1, "Spore Labs");
    repo.insert(&mut s).await.unwrap();
    let mut other = supplier(2, "Other");
    repo.insert(&mut other).await.unwrap();
    let rows = ports.supplier_variants();
    rows.put(&row(1, 10, Some("350"))).await.unwrap();
    rows.put(&row(2, 10, None)).await.unwrap();

    let mut stale = s.clone();
    s.update(full_draft()).unwrap();
    repo.update(&mut s).await.unwrap();
    stale.delete();
    assert_eq!(repo.delete(&mut stale).await, Err(RepoError::StaleVersion));

    s.delete();
    repo.delete(&mut s).await.unwrap();
    assert_eq!(repo.get(sid(1)).await, Ok(None));
    assert_eq!(rows.get(sid(1), vid(10)).await, Ok(None));
    assert_eq!(rows.for_variant(vid(10)).await, Ok(vec![row(2, 10, None)]));
    let last = ports
        .outbox()
        .events_after(0, 100)
        .await
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(last.event_type, "supplier.deleted");
}

async fn lists<S: HasSuppliers>(ports: &S) {
    let repo = ports.suppliers();
    let mut archived = SupplierDraft::new("Old Spores", Currency::EUR);
    archived.archived = true;
    let mut old = Supplier::new(sid(1), archived).unwrap();
    let mut labs = supplier(2, "Spore Labs");
    let mut glass = supplier(3, "Lab Glass Co");
    for s in [&mut old, &mut labs, &mut glass] {
        repo.insert(s).await.unwrap();
    }
    let ids = |page: hypha_core::Page<Supplier>| -> Vec<SupplierId> {
        page.items.iter().map(Supplier::id).collect()
    };

    let all = repo
        .list(&SupplierFilter::default(), page(2, None))
        .await
        .unwrap();
    assert!(all.has_more);
    assert_eq!(ids(all), [sid(3), sid(2)]);
    let rest = repo
        .list(&SupplierFilter::default(), page(2, Some(sid(2))))
        .await
        .unwrap();
    assert!(!rest.has_more);
    assert_eq!(ids(rest), [sid(1)]);

    let filter = SupplierFilter {
        archived: Some(false),
        q: None,
    };
    assert_eq!(
        ids(repo.list(&filter, page(10, None)).await.unwrap()),
        [sid(3), sid(2)]
    );
    let filter = SupplierFilter {
        archived: None,
        q: Some("spore".to_string()),
    };
    assert_eq!(
        ids(repo.list(&filter, page(10, None)).await.unwrap()),
        [sid(2), sid(1)]
    );
    let filter = SupplierFilter {
        archived: Some(true),
        q: Some("SPORE".to_string()),
    };
    assert_eq!(
        ids(repo.list(&filter, page(10, None)).await.unwrap()),
        [sid(1)]
    );
}

async fn with_suppliers<S: HasSuppliers>(ports: &S, n: u128) {
    for i in 1..=n {
        ports
            .suppliers()
            .insert(&mut supplier(i, &format!("Supplier {i}")))
            .await
            .unwrap();
    }
}

async fn put_and_get<S: HasSuppliers + HasSupplierVariants>(ports: &S) {
    with_suppliers(ports, 1).await;
    let rows = ports.supplier_variants();
    let first = row(1, 10, Some("0.35"));
    rows.put(&first).await.unwrap();
    assert_eq!(rows.get(sid(1), vid(10)).await, Ok(Some(first)));
    assert_eq!(rows.get(sid(1), vid(11)).await, Ok(None));

    let bare = SupplierVariant::new(
        sid(1),
        Currency::EUR,
        vid(10),
        SupplierVariantDraft::default(),
    )
    .unwrap();
    rows.put(&bare).await.unwrap();
    assert_eq!(rows.get(sid(1), vid(10)).await, Ok(Some(bare)));
}

async fn missing_supplier<S: HasSuppliers + HasSupplierVariants>(ports: &S) {
    assert_eq!(
        ports.supplier_variants().put(&row(1, 10, None)).await,
        Err(RepoError::Reference(
            hypha_purchasing::keys::CATALOGUE_SUPPLIER
        ))
    );
}

async fn catalogue_pages<S: HasSuppliers + HasSupplierVariants>(ports: &S) {
    with_suppliers(ports, 2).await;
    let rows = ports.supplier_variants();
    for variant in [10, 11, 12] {
        rows.put(&row(1, variant, None)).await.unwrap();
    }
    rows.put(&row(2, 13, None)).await.unwrap();
    let first = rows.list(sid(1), page(2, None)).await.unwrap();
    assert!(first.has_more);
    assert_eq!(first.items, [row(1, 12, None), row(1, 11, None)]);
    let rest = rows.list(sid(1), page(2, Some(vid(11)))).await.unwrap();
    assert!(!rest.has_more);
    assert_eq!(rest.items, [row(1, 10, None)]);
    assert_eq!(
        rows.list(sid(2), page(10, None)).await.unwrap().items,
        [row(2, 13, None)]
    );
}

async fn per_variant<S: HasSuppliers + HasSupplierVariants>(ports: &S) {
    with_suppliers(ports, 3).await;
    let rows = ports.supplier_variants();
    rows.put(&row(3, 10, Some("400"))).await.unwrap();
    rows.put(&row(1, 10, Some("350"))).await.unwrap();
    rows.put(&row(2, 11, None)).await.unwrap();
    assert_eq!(
        rows.for_variant(vid(10)).await,
        Ok(vec![row(1, 10, Some("350")), row(3, 10, Some("400"))])
    );
    assert_eq!(rows.for_variant(vid(12)).await, Ok(vec![]));
}

async fn removing<S: HasSuppliers + HasSupplierVariants>(ports: &S) {
    with_suppliers(ports, 2).await;
    let rows = ports.supplier_variants();
    for (s, v) in [(1, 10), (1, 11), (2, 10), (2, 12)] {
        rows.put(&row(s, v, None)).await.unwrap();
    }
    assert_eq!(rows.remove(sid(1), vid(11)).await, Ok(true));
    assert_eq!(rows.remove(sid(1), vid(11)).await, Ok(false));
    rows.remove_variants(&[vid(10), vid(99)]).await.unwrap();
    rows.remove_variants(&[]).await.unwrap();
    assert_eq!(rows.for_variant(vid(10)).await, Ok(vec![]));
    assert_eq!(rows.get(sid(2), vid(12)).await, Ok(Some(row(2, 12, None))));
}

async fn costs<S: HasSuppliers + HasSupplierVariants>(ports: &S) {
    with_suppliers(ports, 2).await;
    let rows = ports.supplier_variants();
    rows.put(&row(1, 10, None)).await.unwrap();
    rows.put(&row(2, 10, Some("1050"))).await.unwrap();
    assert_eq!(rows.has_costs(sid(1)).await, Ok(false));
    assert_eq!(rows.has_costs(sid(2)).await, Ok(true));
    assert_eq!(rows.has_costs(sid(9)).await, Ok(false));
}
