//! What every discounts store must do: discounts with their rules, codes unique in the
//! store, and usage counted under limits in a batch.

use std::collections::{HashMap, HashSet};

use hypha_core::{
    BasisPoints, CategoryId, ChannelId, Currency, CustomerGroupId, CustomerId, DiscountId,
    HasBatch, MarketId, Money, OrderId, PageLimit, PageRequest, ProductId, RepoError, StageEvents,
    Stamped, Timestamp, VariantId, Version, Versioned,
};
use hypha_discounts::{
    Allocation, Class, ClassSet, CodeFilter, CodeSelection, Condition, Discount, DiscountCode,
    DiscountDraft, DiscountFilter, DiscountRepo, HasDiscounts, ItemSet, Items, Kind, Limits,
    Measure, Method, NameRequest, Names, PerMarket, Phase, Reward, StageDiscounts, Status, Target,
    Threshold, Tier, UsageClaim, Value, keys,
};
use uuid::Uuid;

use super::scenario;

fn did(n: u128) -> DiscountId {
    DiscountId::from_uuid(Uuid::from_u128(n))
}

fn mkt(n: u128) -> MarketId {
    MarketId::from_uuid(Uuid::from_u128(n))
}

fn eur(n: i64) -> Money {
    Money::new(n, Currency::EUR).unwrap()
}

fn bp(n: i32) -> BasisPoints {
    BasisPoints::new(n).unwrap()
}

fn order(n: u128) -> OrderId {
    OrderId::from_uuid(Uuid::from_u128(n))
}

fn customer(n: u128) -> CustomerId {
    CustomerId::from_uuid(Uuid::from_u128(n))
}

fn at(micros: i64) -> Timestamp {
    Timestamp::from_unix_micros(micros)
}

fn percent_off_order() -> Kind {
    Kind::Standard {
        rewards: vec![Reward {
            target: Target::Subtotal,
            value: Value::Percent(bp(1000)),
        }],
    }
}

fn discount(n: u128, method: Method) -> Discount {
    Discount::new(
        did(n),
        DiscountDraft::new("Offer", method, percent_off_order()),
    )
    .unwrap()
}

/// A discount with every kind of rule, to check each survives storage.
fn everything(n: u128) -> Discount {
    let category = CategoryId::from_uuid(Uuid::from_u128(3));
    let draft = DiscountDraft {
        title: "Everything".to_string(),
        method: Method::Codes,
        status: Status::Draft,
        starts_at: Some(at(1_000)),
        ends_at: Some(at(2_000)),
        conditions: vec![
            Condition::Markets(vec![mkt(1)]),
            Condition::Channels(vec![ChannelId::from_uuid(Uuid::from_u128(4))]),
            Condition::CustomerGroups(vec![CustomerGroupId::from_uuid(Uuid::from_u128(5))]),
            Condition::Customers(vec![customer(6)]),
            Condition::SignedIn,
            Condition::MinSubtotal(PerMarket::of(mkt(1), eur(5000))),
            Condition::MinQuantity(2),
        ],
        kind: Kind::Standard {
            rewards: vec![
                Reward {
                    target: Target::Items(ItemSet {
                        items: Items::Categories(vec![category]),
                        exclude_on_sale: true,
                    }),
                    value: Value::Amount {
                        amounts: PerMarket::new([(mkt(1), eur(500)), (mkt(2), eur(600))].into()),
                        allocation: Allocation::Cheapest { units: 2 },
                    },
                },
                Reward {
                    target: Target::Subtotal,
                    value: Value::Percent(bp(1250)),
                },
                Reward {
                    target: Target::Shipping {
                        max_amount: Some(PerMarket::of(mkt(1), eur(1000))),
                    },
                    value: Value::Percent(bp(10_000)),
                },
            ],
        },
        combines_with: ClassSet::of(&[Class::Order, Class::Shipping]),
        limits: Limits {
            total: Some(100),
            once_per_customer: true,
        },
    };
    Discount::new(did(n), draft).unwrap()
}

fn deal(n: u128) -> Discount {
    let kind = Kind::BuyXGetY {
        buys: ItemSet {
            items: Items::Products(vec![ProductId::from_uuid(Uuid::from_u128(1))]),
            exclude_on_sale: false,
        },
        buy: Threshold::Amount(PerMarket::of(mkt(1), eur(2000))),
        gets: ItemSet {
            items: Items::Variants(vec![VariantId::from_uuid(Uuid::from_u128(2))]),
            exclude_on_sale: false,
        },
        get_quantity: 1,
        value: Value::Amount {
            amounts: PerMarket::of(mkt(1), eur(300)),
            allocation: Allocation::Each,
        },
        uses_per_order: Some(3),
    };
    Discount::new(did(n), DiscountDraft::new("Deal", Method::Automatic, kind)).unwrap()
}

fn volume(n: u128) -> Discount {
    let tier = |from: i64, value: Value| Tier {
        threshold: Threshold::Amount(PerMarket::of(mkt(1), eur(from))),
        value,
    };
    let kind = Kind::Tiered {
        items: ItemSet::all(),
        measure: Measure::Subtotal,
        tiers: vec![
            tier(
                5000,
                Value::Amount {
                    amounts: PerMarket::of(mkt(1), eur(500)),
                    allocation: Allocation::Across,
                },
            ),
            tier(10_000, Value::Percent(bp(1500))),
        ],
    };
    Discount::new(
        did(n),
        DiscountDraft::new("Volume", Method::Automatic, kind),
    )
    .unwrap()
}

fn code(discount: u128, text: &str) -> DiscountCode {
    DiscountCode::new(did(discount), text, None, None).unwrap()
}

fn page<Id>(n: u32) -> PageRequest<Id> {
    PageRequest {
        after: None,
        limit: PageLimit::new(n).unwrap(),
    }
}

/// A discount with a total limit of `limit` uses.
fn limited(n: u128, limit: u32) -> Discount {
    let mut draft = DiscountDraft::new("Offer", Method::Automatic, percent_off_order());
    draft.limits.total = Some(limit);
    Discount::new(did(n), draft).unwrap()
}

fn claim(discount: u128, n: u128) -> UsageClaim {
    UsageClaim {
        discount: did(discount),
        code: None,
        order: order(n),
        customer: customer(n),
        once_per_customer: false,
        amount: eur(150),
        at: at(5),
    }
}

/// Runs every discounts scenario, each on fresh ports from `make`.
pub async fn discount_repo<S>(make: impl AsyncFn() -> S)
where
    S: HasDiscounts + HasBatch<Batch: StageDiscounts + StageEvents>,
{
    scenario("discount: every rule survives a round trip, at version 1, stamped");
    round_trip(&make().await).await;
    scenario("discount: buy X get Y and tiers survive a round trip");
    other_kinds(&make().await).await;
    scenario("discount: update bumps the version; a stale copy fails");
    updates(&make().await).await;
    scenario("discount: delete takes its codes, counter and usage; a stale copy fails");
    deletes(&make().await).await;
    scenario("discount: lists filter by phase, method, class, words and code, newest first");
    lists(&make().await).await;
    scenario("discount: live automatic ones are found and counted");
    live(&make().await).await;
    scenario("discount: codes are unique in the store, added all or none");
    codes(&make().await).await;
    scenario("discount: codes page by code, filtered by part and batch, and are counted");
    code_pages(&make().await).await;
    scenario("discount: codes are removed by list and by batch");
    removing(&make().await).await;
    scenario("discount: codes are made inactive and active again by list and by batch");
    activating(&make().await).await;
    scenario("discount: a discount and its codes commit together, or not at all");
    staged(&make().await).await;
    scenario("discount: a claim counts a use under the discount's and the code's limits");
    claims(&make().await).await;
    scenario("discount: a claim needs the discount live and the code active at the commit");
    claims_at_commit(&make().await).await;
    scenario("discount: uses without a limit are counted; a limit set later counts them");
    unlimited(&make().await).await;
    scenario("discount: two checkouts race for the last use: one wins, one is refused");
    last_use(&make().await).await;
    scenario("discount: once per customer refuses a second use by the same customer");
    once_per_customer(&make().await).await;
    scenario("discount: active ones with a boundary ahead are found, by ID");
    with_boundaries(&make().await).await;
    scenario("discount schedule: none before the first run, moved only from where it was");
    schedule(&make().await).await;
}

async fn with_boundaries<S: HasDiscounts>(ports: &S) {
    let repo = ports.discounts();
    let windowed = |n: u128, status: Status, starts: Option<i64>, ends: Option<i64>| {
        Discount::new(
            did(n),
            DiscountDraft {
                status,
                starts_at: starts.map(at),
                ends_at: ends.map(at),
                ..DiscountDraft::new("Offer", Method::Codes, percent_off_order())
            },
        )
        .unwrap()
    };
    for mut d in [
        windowed(4, Status::Active, None, Some(3_000)),
        windowed(1, Status::Active, Some(1_000), Some(2_000)),
        windowed(2, Status::Active, Some(500), None),
        windowed(3, Status::Draft, Some(2_500), None),
        windowed(5, Status::Active, None, None),
    ] {
        repo.insert(&mut d).await.unwrap();
    }
    let ids = |found: Vec<Discount>| found.iter().map(|d| d.id()).collect::<Vec<_>>();
    assert_eq!(
        repo.with_boundary_after(at(500)).await.map(ids),
        Ok(vec![did(1), did(4)])
    );
    // A boundary at `after` is behind it.
    assert_eq!(
        repo.with_boundary_after(at(2_000)).await.map(ids),
        Ok(vec![did(4)])
    );
    assert_eq!(
        repo.with_boundary_after(at(3_000)).await.map(ids),
        Ok(vec![])
    );
}

async fn schedule<S: HasDiscounts + HasBatch<Batch: StageDiscounts>>(ports: &S) {
    let repo = ports.discounts();
    assert_eq!(repo.schedule_ran_until().await, Ok(None));
    let mut batch = ports.batch();
    batch.advance_discount_schedule(None, at(1_000));
    ports.commit(batch).await.unwrap();
    assert_eq!(repo.schedule_ran_until().await, Ok(Some(at(1_000))));
    let mut batch = ports.batch();
    batch.advance_discount_schedule(Some(at(1_000)), at(2_000));
    ports.commit(batch).await.unwrap();
    assert_eq!(repo.schedule_ran_until().await, Ok(Some(at(2_000))));
    for from in [None, Some(at(1_000))] {
        let mut batch = ports.batch();
        batch.advance_discount_schedule(from, at(3_000));
        assert_eq!(ports.commit(batch).await, Err(RepoError::StaleVersion));
    }
    assert_eq!(repo.schedule_ran_until().await, Ok(Some(at(2_000))));
}

async fn round_trip<S: HasDiscounts>(ports: &S) {
    let repo = ports.discounts();
    let mut d = everything(1);
    repo.insert(&mut d).await.unwrap();
    assert_eq!(d.version(), Version::new(1));
    let stored = repo.get(did(1)).await.unwrap().unwrap();
    assert_eq!(stored, d);
    assert!(stored.stamps().is_some());
    assert_eq!(repo.get(did(9)).await, Ok(None));
    assert_eq!(repo.get_many(&[did(9), did(1)]).await, Ok(vec![d]));
    assert_eq!(
        repo.insert(&mut everything(1)).await,
        Err(RepoError::Duplicate(keys::DISCOUNT_ID))
    );
}

async fn other_kinds<S: HasDiscounts>(ports: &S) {
    let repo = ports.discounts();
    let mut a = deal(1);
    let mut b = volume(2);
    repo.insert(&mut a).await.unwrap();
    repo.insert(&mut b).await.unwrap();
    assert_eq!(repo.get_many(&[did(1), did(2)]).await, Ok(vec![a, b]));
}

async fn updates<S: HasDiscounts>(ports: &S) {
    let repo = ports.discounts();
    let mut d = discount(1, Method::Codes);
    repo.insert(&mut d).await.unwrap();
    let stale = d.clone();
    let mut draft = d.draft();
    draft.title = "Renamed".to_string();
    draft.limits.total = Some(5);
    d.update(draft).unwrap();
    repo.update(&mut d).await.unwrap();
    assert_eq!(d.version(), Version::new(2));
    assert_eq!(repo.get(did(1)).await.unwrap(), Some(d));
    let mut stale = stale;
    let mut draft = stale.draft();
    draft.title = "Late".to_string();
    stale.update(draft).unwrap();
    assert_eq!(repo.update(&mut stale).await, Err(RepoError::StaleVersion));
    assert_eq!(
        repo.update(&mut discount(9, Method::Codes)).await,
        Err(RepoError::StaleVersion)
    );
}

async fn deletes<S: HasDiscounts + HasBatch<Batch: StageDiscounts>>(ports: &S) {
    let repo = ports.discounts();
    let mut d = discount(1, Method::Codes);
    repo.insert(&mut d).await.unwrap();
    repo.add_codes(&[code(1, "GONE")]).await.unwrap();
    let mut batch = ports.batch();
    batch.claim_discount_usage(claim(1, 1));
    ports.commit(batch).await.unwrap();
    let mut stale = d.clone();

    d.delete();
    repo.delete(&mut d).await.unwrap();
    assert_eq!(repo.get(did(1)).await, Ok(None));
    assert_eq!(repo.find_codes(&["GONE".to_string()]).await, Ok(vec![]));
    assert_eq!(repo.usage_counts(&[did(1)]).await, Ok(HashMap::new()));
    assert!(
        repo.usage_page(did(1), page(10))
            .await
            .unwrap()
            .items
            .is_empty()
    );
    stale.delete();
    assert_eq!(repo.delete(&mut stale).await, Err(RepoError::StaleVersion));
    // The code is free again.
    let mut again = discount(2, Method::Codes);
    repo.insert(&mut again).await.unwrap();
    assert_eq!(repo.add_codes(&[code(2, "gone")]).await, Ok(()));
}

async fn lists<S: HasDiscounts>(ports: &S) {
    let repo = ports.discounts();
    let mut codes = discount(1, Method::Codes);
    let mut auto = Discount::new(
        did(2),
        DiscountDraft::new(
            "Free shipping",
            Method::Automatic,
            Kind::Standard {
                rewards: vec![Reward {
                    target: Target::Shipping { max_amount: None },
                    value: Value::Percent(bp(10_000)),
                }],
            },
        ),
    )
    .unwrap();
    let mut draft = {
        let mut x = DiscountDraft::new("Summer sale", Method::Codes, percent_off_order());
        x.status = Status::Draft;
        Discount::new(did(3), x).unwrap()
    };
    let mut ended = {
        let mut x = DiscountDraft::new("Old", Method::Automatic, percent_off_order());
        x.ends_at = Some(at(10));
        Discount::new(did(4), x).unwrap()
    };
    for d in [&mut codes, &mut auto, &mut draft, &mut ended] {
        repo.insert(d).await.unwrap();
    }
    repo.add_codes(&[code(1, "WELCOME10"), code(3, "SUN")])
        .await
        .unwrap();

    let ids = |filter: DiscountFilter| async move {
        repo.list(&filter, page(10))
            .await
            .unwrap()
            .items
            .iter()
            .map(Discount::id)
            .collect::<Vec<_>>()
    };
    let now = at(100);
    let all = DiscountFilter::everything(now);
    assert_eq!(ids(all.clone()).await, vec![did(4), did(3), did(2), did(1)]);
    assert_eq!(
        ids(DiscountFilter {
            phase: Some(Phase::Expired),
            ..all.clone()
        })
        .await,
        vec![did(4)]
    );
    assert_eq!(
        ids(DiscountFilter {
            phase: Some(Phase::Draft),
            ..all.clone()
        })
        .await,
        vec![did(3)]
    );
    assert_eq!(
        ids(DiscountFilter {
            phase: Some(Phase::Active),
            method: Some(Method::Automatic),
            ..all.clone()
        })
        .await,
        vec![did(2)]
    );
    assert_eq!(
        ids(DiscountFilter {
            class: Some(Class::Shipping),
            ..all.clone()
        })
        .await,
        vec![did(2)]
    );
    // Words: in the title ignoring case, or a part of a code.
    assert_eq!(
        ids(DiscountFilter {
            q: Some("summer".to_string()),
            ..all.clone()
        })
        .await,
        vec![did(3)]
    );
    assert_eq!(
        ids(DiscountFilter {
            q: Some("come1".to_string()),
            ..all.clone()
        })
        .await,
        vec![did(1)]
    );
    assert_eq!(
        ids(DiscountFilter {
            code: Some("SUN".to_string()),
            ..all.clone()
        })
        .await,
        vec![did(3)]
    );
    let first = repo.list(&all, page(2)).await.unwrap();
    assert!(first.has_more);
    let rest = repo
        .list(
            &all,
            PageRequest {
                after: Some(did(3)),
                limit: PageLimit::new(10).unwrap(),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        rest.items.iter().map(Discount::id).collect::<Vec<_>>(),
        vec![did(2), did(1)]
    );
}

async fn live<S: HasDiscounts>(ports: &S) {
    let repo = ports.discounts();
    let scheduled = |n, start| {
        let mut x = DiscountDraft::new("Later", Method::Automatic, percent_off_order());
        x.starts_at = Some(at(start));
        Discount::new(did(n), x).unwrap()
    };
    let mut now = discount(1, Method::Automatic);
    let mut later = scheduled(2, 500);
    let mut coded = discount(3, Method::Codes);
    let mut ended = {
        let mut x = DiscountDraft::new("Old", Method::Automatic, percent_off_order());
        x.ends_at = Some(at(50));
        Discount::new(did(4), x).unwrap()
    };
    let mut paused = {
        let mut x = DiscountDraft::new("Paused", Method::Automatic, percent_off_order());
        x.status = Status::Draft;
        Discount::new(did(5), x).unwrap()
    };
    for d in [&mut now, &mut later, &mut coded, &mut ended, &mut paused] {
        repo.insert(d).await.unwrap();
    }
    assert_eq!(repo.live_automatic(at(100)).await, Ok(vec![now.clone()]));
    // Scheduled ones count: they will be live.
    assert_eq!(repo.count_live_automatic(at(100)).await, Ok(2));
    // A window's start is inclusive and its end exclusive.
    assert_eq!(
        repo.live_automatic(at(500)).await.unwrap().len(),
        2,
        "starts at 500"
    );
    assert_eq!(repo.live_automatic(at(50)).await.unwrap().len(), 1);
}

async fn codes<S: HasDiscounts>(ports: &S) {
    let repo = ports.discounts();
    repo.insert(&mut discount(1, Method::Codes)).await.unwrap();
    repo.insert(&mut discount(2, Method::Codes)).await.unwrap();
    let mut limited =
        DiscountCode::new(did(1), "Single", Some(1), Some("Newsletter".to_string())).unwrap();
    repo.add_codes(&[limited.clone(), code(1, "OTHER")])
        .await
        .unwrap();
    let found = repo
        .find_codes(&["SINGLE".to_string(), "NOPE".to_string()])
        .await
        .unwrap();
    assert_eq!(found.len(), 1);
    limited.created_at = found[0].created_at;
    assert!(limited.created_at.is_some());
    assert_eq!(found, vec![limited]);
    // Another discount can't take it, and nothing of a refused set is added.
    assert_eq!(
        repo.add_codes(&[code(2, "FRESH"), code(2, "single")]).await,
        Err(RepoError::Duplicate(keys::DISCOUNT_CODE))
    );
    assert_eq!(repo.find_codes(&["FRESH".to_string()]).await, Ok(vec![]));
    assert_eq!(
        repo.code_counts(&[did(1), did(2)]).await,
        Ok(HashMap::from([(did(1), 2)]))
    );
    assert!(matches!(
        repo.add_codes(&[code(9, "ORPHAN")]).await,
        Err(RepoError::Backend(_))
    ));
}

async fn code_pages<S: HasDiscounts>(ports: &S) {
    let repo = ports.discounts();
    repo.insert(&mut discount(1, Method::Codes)).await.unwrap();
    let batch =
        |text: &str| DiscountCode::new(did(1), text, Some(1), Some("Spring".to_string())).unwrap();
    repo.add_codes(&[code(1, "C3"), batch("A1"), batch("B2")])
        .await
        .unwrap();
    let texts =
        |found: Vec<DiscountCode>| found.into_iter().map(|c| c.normalized).collect::<Vec<_>>();
    let first = repo
        .code_page(did(1), &CodeFilter::default(), page(2))
        .await
        .unwrap();
    assert!(first.has_more);
    assert_eq!(texts(first.items), vec!["A1", "B2"]);
    let rest = repo
        .code_page(
            did(1),
            &CodeFilter::default(),
            PageRequest {
                after: Some("B2".to_string()),
                limit: PageLimit::new(2).unwrap(),
            },
        )
        .await
        .unwrap();
    assert_eq!(texts(rest.items), vec!["C3"]);
    let spring = CodeFilter {
        q: None,
        batch: Some("Spring".to_string()),
        active: None,
    };
    assert_eq!(
        texts(
            repo.code_page(did(1), &spring, page(10))
                .await
                .unwrap()
                .items
        ),
        vec!["A1", "B2"]
    );
    let part = CodeFilter {
        q: Some("2".to_string()),
        batch: None,
        active: None,
    };
    assert_eq!(
        texts(repo.code_page(did(1), &part, page(10)).await.unwrap().items),
        vec!["B2"]
    );
    assert_eq!(repo.count_codes(did(1), &spring).await, Ok(2));
    assert_eq!(
        repo.count_codes(did(1), &CodeFilter::default()).await,
        Ok(3)
    );
}

async fn removing<S: HasDiscounts + HasBatch<Batch: StageDiscounts>>(ports: &S) {
    let repo = ports.discounts();
    repo.insert(&mut discount(1, Method::Codes)).await.unwrap();
    repo.insert(&mut discount(2, Method::Codes)).await.unwrap();
    let spring = |d: u128, text: &str| {
        DiscountCode::new(did(d), text, None, Some("Spring".to_string())).unwrap()
    };
    repo.add_codes(&[
        code(1, "KEEP"),
        code(1, "DROP"),
        spring(1, "S1"),
        spring(1, "S2"),
        spring(2, "OTHER"),
    ])
    .await
    .unwrap();
    let mut batch = ports.batch();
    batch.remove_discount_codes(did(1), CodeSelection::Codes(vec!["DROP".to_string()]));
    batch.remove_discount_codes(did(1), CodeSelection::Batch("Spring".to_string()));
    // Another discount's code isn't removed by naming it.
    batch.remove_discount_codes(did(1), CodeSelection::Codes(vec!["OTHER".to_string()]));
    ports.commit(batch).await.unwrap();
    let left: HashSet<String> = repo
        .find_codes(&[
            "KEEP".to_string(),
            "DROP".to_string(),
            "S1".to_string(),
            "S2".to_string(),
            "OTHER".to_string(),
        ])
        .await
        .unwrap()
        .into_iter()
        .map(|c| c.normalized)
        .collect();
    assert_eq!(
        left,
        HashSet::from(["KEEP".to_string(), "OTHER".to_string()])
    );
}

async fn activating<S: HasDiscounts + HasBatch<Batch: StageDiscounts>>(ports: &S) {
    let repo = ports.discounts();
    repo.insert(&mut discount(1, Method::Codes)).await.unwrap();
    repo.insert(&mut discount(2, Method::Codes)).await.unwrap();
    let spring = |d: u128, text: &str| {
        DiscountCode::new(did(d), text, None, Some("Spring".to_string())).unwrap()
    };
    repo.add_codes(&[
        code(1, "KEEP"),
        code(1, "OFF"),
        spring(1, "S1"),
        spring(1, "S2"),
        spring(2, "OTHER"),
    ])
    .await
    .unwrap();
    let mut batch = ports.batch();
    batch.set_discount_codes_active(did(1), CodeSelection::Codes(vec!["OFF".to_string()]), false);
    batch.set_discount_codes_active(did(1), CodeSelection::Batch("Spring".to_string()), false);
    // Another discount's code isn't changed by naming it.
    batch.set_discount_codes_active(
        did(1),
        CodeSelection::Codes(vec!["OTHER".to_string()]),
        false,
    );
    ports.commit(batch).await.unwrap();
    let inactive = CodeFilter {
        active: Some(false),
        ..CodeFilter::default()
    };
    let found = repo.code_page(did(1), &inactive, page(10)).await.unwrap();
    let texts: Vec<String> = found.items.into_iter().map(|c| c.normalized).collect();
    assert_eq!(texts, vec!["OFF", "S1", "S2"]);
    assert_eq!(repo.count_codes(did(1), &inactive).await, Ok(3));
    let other = repo.find_codes(&["OTHER".to_string()]).await.unwrap();
    assert!(other.iter().all(|c| c.active));
    let mut batch = ports.batch();
    batch.set_discount_codes_active(did(1), CodeSelection::Batch("Spring".to_string()), true);
    ports.commit(batch).await.unwrap();
    assert_eq!(repo.count_codes(did(1), &inactive).await, Ok(1));
}

async fn staged<S: HasDiscounts + HasBatch<Batch: StageDiscounts + StageEvents>>(ports: &S) {
    let repo = ports.discounts();
    repo.insert(&mut discount(1, Method::Codes)).await.unwrap();
    repo.add_codes(&[code(1, "TAKEN")]).await.unwrap();

    let mut refused = discount(2, Method::Codes);
    let mut batch = ports.batch();
    batch.insert_discount(&mut refused);
    batch.add_discount_codes(&[code(2, "NEW"), code(2, "TAKEN")]);
    assert_eq!(
        ports.commit(batch).await,
        Err(RepoError::Duplicate(keys::DISCOUNT_CODE))
    );
    assert_eq!(repo.get(did(2)).await, Ok(None));
    assert_eq!(repo.find_codes(&["NEW".to_string()]).await, Ok(vec![]));

    let mut fresh = discount(3, Method::Codes);
    let mut batch = ports.batch();
    batch.insert_discount(&mut fresh);
    batch.add_discount_codes(&[code(3, "NEW")]);
    ports.commit(batch).await.unwrap();
    assert_eq!(fresh.version(), Version::new(1));
    assert_eq!(repo.get(did(3)).await.unwrap(), Some(fresh));
    assert_eq!(
        repo.find_codes(&["NEW".to_string()]).await.unwrap().len(),
        1
    );
    // Its counter starts at zero: a claim finds it.
    let mut batch = ports.batch();
    batch.claim_discount_usage(claim(3, 1));
    assert_eq!(ports.commit(batch).await, Ok(()));
}

async fn claims<S: HasDiscounts + HasBatch<Batch: StageDiscounts>>(ports: &S) {
    let repo = ports.discounts();
    repo.insert(&mut discount(1, Method::Codes)).await.unwrap();
    repo.add_codes(&[DiscountCode::new(did(1), "ONCE", Some(1), None).unwrap()])
        .await
        .unwrap();

    let mut with_code = claim(1, 1);
    with_code.code = Some(("ONCE".to_string(), Some(1)));
    let mut batch = ports.batch();
    batch.claim_discount_usage(with_code.clone());
    ports.commit(batch).await.unwrap();
    assert_eq!(
        repo.usage_counts(&[did(1)]).await,
        Ok(HashMap::from([(did(1), 1)]))
    );
    assert_eq!(
        repo.find_codes(&["ONCE".to_string()]).await.unwrap()[0].used,
        1
    );
    let usages = repo.usage_page(did(1), page(10)).await.unwrap().items;
    assert_eq!(usages.len(), 1);
    assert_eq!(
        (
            usages[0].order,
            usages[0].customer,
            usages[0].code.as_deref(),
            usages[0].amount,
            usages[0].at
        ),
        (order(1), customer(1), Some("ONCE"), eur(150), at(5))
    );
    assert_eq!(
        repo.used_by(&[did(1)], customer(1)).await,
        Ok(HashSet::from([did(1)]))
    );
    assert_eq!(
        repo.used_by(&[did(1)], customer(2)).await,
        Ok(HashSet::new())
    );
    // Totals per currency, by code: two in euros, one in dollars.
    repo.insert(&mut discount(2, Method::Codes)).await.unwrap();
    let mut batch = ports.batch();
    batch.claim_discount_usage(claim(2, 2));
    batch.claim_discount_usage(claim(2, 3));
    batch.claim_discount_usage(UsageClaim {
        amount: Money::new(70, Currency::USD).unwrap(),
        ..claim(2, 4)
    });
    ports.commit(batch).await.unwrap();
    assert_eq!(
        repo.discounted_totals(&[did(1), did(2), did(9)]).await,
        Ok(HashMap::from([
            (did(1), vec![eur(150)]),
            (
                did(2),
                vec![eur(300), Money::new(70, Currency::USD).unwrap()]
            ),
        ]))
    );

    // The code's own limit is reached: nothing of the batch is written.
    let mut again = claim(1, 2);
    again.code = Some(("ONCE".to_string(), Some(1)));
    let mut batch = ports.batch();
    batch.claim_discount_usage(again);
    assert_eq!(
        ports.commit(batch).await,
        Err(RepoError::Duplicate(keys::DISCOUNT_USAGE_LIMIT))
    );
    assert_eq!(
        repo.usage_counts(&[did(1)]).await,
        Ok(HashMap::from([(did(1), 1)]))
    );
    // A gone discount is refused like a reached limit.
    let mut batch = ports.batch();
    batch.claim_discount_usage(claim(9, 3));
    assert_eq!(
        ports.commit(batch).await,
        Err(RepoError::Duplicate(keys::DISCOUNT_USAGE_LIMIT))
    );
    // Newest order first.
    let mut batch = ports.batch();
    batch.claim_discount_usage(claim(1, 4));
    ports.commit(batch).await.unwrap();
    let orders: Vec<OrderId> = repo
        .usage_page(did(1), page(10))
        .await
        .unwrap()
        .items
        .iter()
        .map(|u| u.order)
        .collect();
    assert_eq!(orders, vec![order(4), order(1)]);
}

async fn claims_at_commit<S: HasDiscounts + HasBatch<Batch: StageDiscounts>>(ports: &S) {
    let repo = ports.discounts();
    let mut d = discount(1, Method::Codes);
    repo.insert(&mut d).await.unwrap();
    repo.add_codes(&[
        code(1, "OFF"),
        DiscountCode::new(did(1), "ONE", Some(5), None).unwrap(),
    ])
    .await
    .unwrap();
    let by = |n: u128, text: &str, limit: Option<u32>| UsageClaim {
        code: Some((text.to_string(), limit)),
        ..claim(1, n)
    };
    // Codes deactivated after the cart was priced, with and without a limit of their own.
    let mut batch = ports.batch();
    batch.set_discount_codes_active(
        did(1),
        CodeSelection::Codes(vec!["OFF".to_string(), "ONE".to_string()]),
        false,
    );
    ports.commit(batch).await.unwrap();
    for (text, limit) in [("OFF", None), ("ONE", Some(5))] {
        let mut batch = ports.batch();
        batch.claim_discount_usage(by(1, text, limit));
        assert_eq!(
            ports.commit(batch).await,
            Err(RepoError::Duplicate(keys::DISCOUNT_USAGE_LIMIT))
        );
    }
    // The discount made a draft, or ended before the claim's moment.
    let mut draft = d.draft();
    draft.status = Status::Draft;
    d.update(draft).unwrap();
    repo.update(&mut d).await.unwrap();
    let mut batch = ports.batch();
    batch.claim_discount_usage(claim(1, 2));
    assert_eq!(
        ports.commit(batch).await,
        Err(RepoError::Duplicate(keys::DISCOUNT_USAGE_LIMIT))
    );
    let mut draft = d.draft();
    draft.status = Status::Active;
    draft.ends_at = Some(at(5));
    d.update(draft).unwrap();
    repo.update(&mut d).await.unwrap();
    let mut batch = ports.batch();
    batch.claim_discount_usage(claim(1, 3));
    assert_eq!(
        ports.commit(batch).await,
        Err(RepoError::Duplicate(keys::DISCOUNT_USAGE_LIMIT))
    );
    assert_eq!(repo.usage_counts(&[did(1)]).await, Ok(HashMap::new()));
}

async fn unlimited<S: HasDiscounts + HasBatch<Batch: StageDiscounts>>(ports: &S) {
    let repo = ports.discounts();
    let mut d = discount(1, Method::Codes);
    repo.insert(&mut d).await.unwrap();
    repo.add_codes(&[code(1, "ANY")]).await.unwrap();
    let mut batch = ports.batch();
    for n in 1..=2 {
        batch.claim_discount_usage(UsageClaim {
            code: Some(("ANY".to_string(), None)),
            ..claim(1, n)
        });
    }
    ports.commit(batch).await.unwrap();
    assert_eq!(
        repo.usage_counts(&[did(1)]).await,
        Ok(HashMap::from([(did(1), 2)]))
    );
    assert_eq!(
        repo.find_codes(&["ANY".to_string()]).await.unwrap()[0].used,
        2
    );
    // Given a limit of 3 after two uses, it has one left.
    let mut draft = d.draft();
    draft.limits.total = Some(3);
    d.update(draft).unwrap();
    repo.update(&mut d).await.unwrap();
    let mut batch = ports.batch();
    batch.claim_discount_usage(claim(1, 3));
    ports.commit(batch).await.unwrap();
    let mut batch = ports.batch();
    batch.claim_discount_usage(claim(1, 4));
    assert_eq!(
        ports.commit(batch).await,
        Err(RepoError::Duplicate(keys::DISCOUNT_USAGE_LIMIT))
    );
    assert_eq!(
        repo.usage_counts(&[did(1)]).await,
        Ok(HashMap::from([(did(1), 3)]))
    );
}

async fn last_use<S: HasDiscounts + HasBatch<Batch: StageDiscounts>>(ports: &S) {
    let repo = ports.discounts();
    repo.insert(&mut limited(1, 1)).await.unwrap();
    let mut first = ports.batch();
    first.claim_discount_usage(claim(1, 1));
    let mut second = ports.batch();
    second.claim_discount_usage(claim(1, 2));
    // Both read the counter at zero; only one may move it to the limit.
    assert_eq!(ports.commit(first).await, Ok(()));
    assert_eq!(
        ports.commit(second).await,
        Err(RepoError::Duplicate(keys::DISCOUNT_USAGE_LIMIT))
    );
    assert_eq!(
        repo.usage_counts(&[did(1)]).await,
        Ok(HashMap::from([(did(1), 1)]))
    );
}

async fn once_per_customer<S: HasDiscounts + HasBatch<Batch: StageDiscounts>>(ports: &S) {
    let repo = ports.discounts();
    repo.insert(&mut discount(1, Method::Automatic))
        .await
        .unwrap();
    let once = |n: u128, who: u128| UsageClaim {
        once_per_customer: true,
        customer: customer(who),
        ..claim(1, n)
    };
    let mut batch = ports.batch();
    batch.claim_discount_usage(once(1, 7));
    ports.commit(batch).await.unwrap();
    let mut batch = ports.batch();
    batch.claim_discount_usage(once(2, 7));
    assert_eq!(
        ports.commit(batch).await,
        Err(RepoError::Duplicate(keys::DISCOUNT_PER_CUSTOMER))
    );
    // Another customer may, and so may the same one where the discount isn't once each.
    let mut batch = ports.batch();
    batch.claim_discount_usage(once(3, 8));
    batch.claim_discount_usage(UsageClaim {
        customer: customer(7),
        ..claim(1, 4)
    });
    ports.commit(batch).await.unwrap();
    assert_eq!(
        repo.usage_counts(&[did(1)]).await,
        Ok(HashMap::from([(did(1), 3)]))
    );
}

/// Runs the scenario of `DiscountRepo::names`, which reads the other domains' tables.
pub async fn discount_names<S>(make: impl AsyncFn() -> S)
where
    S: HasDiscounts
        + hypha_products::HasProducts
        + hypha_categories::HasCategories
        + hypha_markets::HasMarkets
        + hypha_channels::HasChannels
        + hypha_customers::HasCustomerGroups
        + hypha_customers::HasCustomers,
{
    scenario("discount names: what exists is named, a variant with its options, in one read");
    names(&make().await).await;
    scenario("discount names: for shoppers, only active products and their variants");
    active_names(&make().await).await;
}

async fn names<S>(ports: &S)
where
    S: HasDiscounts
        + hypha_products::HasProducts
        + hypha_categories::HasCategories
        + hypha_markets::HasMarkets
        + hypha_channels::HasChannels
        + hypha_customers::HasCustomerGroups
        + hypha_customers::HasCustomers,
{
    use hypha_categories::CategoryRepo;
    use hypha_channels::ChannelRepo;
    use hypha_customers::{CustomerGroupRepo, CustomerRepo};
    use hypha_markets::MarketRepo;
    use hypha_products::ProductRepo;

    // Product 1 is simple (variant 101); product 2 is a shirt (S / Red 201, M / Blue 202).
    let mut simple = super::products::simple(1, "mug");
    ports.products().insert(&mut simple).await.unwrap();
    let mut shirt = super::products::shirt(2, "shirt");
    ports.products().insert(&mut shirt).await.unwrap();
    let mut category = super::categories::category(1, None, 0);
    ports.categories().insert(&mut category).await.unwrap();
    let mut market = hypha_markets::Market::new(
        mkt(1),
        "Serbia".to_string(),
        vec![hypha_core::CountryCode::new("RS").unwrap()],
        Currency::new("RSD").unwrap(),
    )
    .unwrap();
    ports.markets().insert(&mut market).await.unwrap();
    let mut channel = hypha_channels::Channel::new(
        ChannelId::from_uuid(Uuid::from_u128(1)),
        "Wholesale portal",
        hypha_channels::ChannelKind::Custom,
    )
    .unwrap();
    ports.channels().insert(&mut channel).await.unwrap();
    let group_id = CustomerGroupId::from_uuid(Uuid::from_u128(1));
    let mut group = hypha_customers::CustomerGroup::new(
        group_id,
        "VIP",
        hypha_core::Handle::new("vip").unwrap(),
        None,
    )
    .unwrap();
    ports.customer_groups().insert(&mut group).await.unwrap();
    let customer = |n: u128| CustomerId::from_uuid(Uuid::from_u128(n));
    let mut named = hypha_customers::Customer::new(
        customer(1),
        hypha_core::Email::new("ana@example.rs").unwrap(),
    );
    named.set_first_name(Some("Ana".to_string())).unwrap();
    named.set_last_name(Some("Petrović".to_string())).unwrap();
    ports.customers().insert(&mut named).await.unwrap();
    let mut first_only = hypha_customers::Customer::new(
        customer(2),
        hypha_core::Email::new("marko@example.rs").unwrap(),
    );
    first_only
        .set_first_name(Some("Marko".to_string()))
        .unwrap();
    ports.customers().insert(&mut first_only).await.unwrap();
    // Known only by their email, which a summary never shows.
    let mut unnamed = hypha_customers::Customer::new(
        customer(3),
        hypha_core::Email::new("anon@example.rs").unwrap(),
    );
    ports.customers().insert(&mut unnamed).await.unwrap();

    let pid = |n: u128| ProductId::from_uuid(Uuid::from_u128(n));
    let vid = |n: u128| VariantId::from_uuid(Uuid::from_u128(n));
    let cat = |n: u128| CategoryId::from_uuid(Uuid::from_u128(n));
    let request = NameRequest {
        products: vec![pid(1), pid(9)],
        variants: vec![vid(101), vid(201), vid(202), vid(909)],
        categories: vec![cat(1), cat(9)],
        markets: vec![mkt(1), mkt(9)],
        channels: vec![ChannelId::from_uuid(Uuid::from_u128(1))],
        customer_groups: vec![group_id],
        customers: vec![customer(1), customer(2), customer(3)],
        active_products_only: false,
    };
    let mut expected = Names::new();
    expected.products.insert(pid(1), "Product 1".to_string());
    expected.variants.insert(vid(101), "Product 1".to_string());
    expected
        .variants
        .insert(vid(201), "Shirt (S / Red)".to_string());
    expected
        .variants
        .insert(vid(202), "Shirt (M / Blue)".to_string());
    expected.categories.insert(cat(1), "Category 1".to_string());
    expected.markets.insert(mkt(1), "Serbia".to_string());
    expected.channels.insert(
        ChannelId::from_uuid(Uuid::from_u128(1)),
        "Wholesale portal".to_string(),
    );
    expected.customer_groups.insert(group_id, "VIP".to_string());
    expected
        .customers
        .insert(customer(1), "Ana Petrović".to_string());
    expected.customers.insert(customer(2), "Marko".to_string());
    assert_eq!(ports.discounts().names(&request).await, Ok(expected));
    assert_eq!(
        ports.discounts().names(&NameRequest::default()).await,
        Ok(Names::new())
    );
}

async fn active_names<S: HasDiscounts + hypha_products::HasProducts>(ports: &S) {
    use hypha_products::{ProductRepo, ProductStatus};

    let mut draft = super::products::simple(1, "draft");
    ports.products().insert(&mut draft).await.unwrap();
    let mut active = super::products::simple(2, "active");
    active.set_status(ProductStatus::Active).unwrap();
    ports.products().insert(&mut active).await.unwrap();
    let pid = |n: u128| ProductId::from_uuid(Uuid::from_u128(n));
    let vid = |n: u128| VariantId::from_uuid(Uuid::from_u128(n));
    let request = NameRequest {
        products: vec![pid(1), pid(2)],
        variants: vec![vid(101), vid(201)],
        active_products_only: true,
        ..NameRequest::default()
    };
    let mut expected = Names::new();
    expected.products.insert(pid(2), "Product 2".to_string());
    expected.variants.insert(vid(201), "Product 2".to_string());
    assert_eq!(ports.discounts().names(&request).await, Ok(expected));
}
