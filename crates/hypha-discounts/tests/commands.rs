//! Discount commands over a store in memory.

use std::collections::HashMap;

use hypha_core::{
    Actor, AuthError, BasisPoints, Ctx, Currency, DiscountId, MarketId, Money, PageLimit,
    PageRequest, Scope, Timestamp, UserId, Versioned,
};
use hypha_discounts::{
    CodeError, CodeFilter, CodeSelection, CommandError, CreateDiscount, DiscountDraft,
    DiscountError, DiscountFilter, DiscountPatch, DiscountRepo, Generation, Kind,
    MAX_LIVE_AUTOMATIC, Method, NewCode, PerMarket, Reward, Status, Target, Value, add_codes,
    create_discount, delete_discount, find_live_code, generate_codes, get_discount, list_codes,
    list_discounts, remove_codes, update_discount,
};
use hypha_testkit::{InMemoryPorts, SequentialIds, TestEnv};
use pollster::block_on;
use uuid::Uuid;

fn mkt() -> MarketId {
    MarketId::from_uuid(Uuid::from_u128(900))
}

fn percent_off_order() -> Kind {
    Kind::Standard {
        rewards: vec![Reward {
            target: Target::Subtotal,
            value: Value::Percent(BasisPoints::new(1000).unwrap()),
        }],
    }
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

    fn currencies() -> HashMap<MarketId, Currency> {
        HashMap::from([(mkt(), Currency::EUR)])
    }

    fn create(&self, method: Method, codes: &[&str]) -> Result<DiscountId, CommandError> {
        let input = CreateDiscount {
            draft: DiscountDraft::new("Welcome", method, percent_off_order()),
            codes: codes
                .iter()
                .map(|c| NewCode {
                    code: c.to_string(),
                    usage_limit: None,
                })
                .collect(),
            currencies: Self::currencies(),
        };
        block_on(create_discount(&self.ctx, &self.ports, &self.ids, input)).map(|d| d.id())
    }

    fn events(&self, event_type: &str) -> Vec<serde_json::Value> {
        self.ports
            .outbox
            .events()
            .into_iter()
            .filter(|e| e.event_type == event_type)
            .map(|e| e.data)
            .collect()
    }

    fn codes_of(&self, id: DiscountId) -> Vec<String> {
        block_on(list_codes(
            &self.ctx,
            &self.ports,
            id,
            CodeFilter::default(),
            page(),
        ))
        .unwrap()
        .items
        .into_iter()
        .map(|c| c.normalized)
        .collect()
    }
}

fn page<Id>() -> PageRequest<Id> {
    PageRequest {
        after: None,
        limit: PageLimit::new(100).unwrap(),
    }
}

#[test]
fn a_discount_is_created_with_its_codes_in_one_write() {
    let w = World::new();
    let id = w.create(Method::Codes, &["welcome10"]).unwrap();
    assert_eq!(w.codes_of(id), vec!["WELCOME10"]);
    assert_eq!(w.events("discount.created").len(), 1);
    assert_eq!(
        w.events("discount.codes_added"),
        vec![serde_json::json!({"discount": id, "count": 1, "batch": null})]
    );
}

#[test]
fn writing_needs_discounts_write() {
    let w = World {
        ctx: Ctx::new(
            Actor::User(UserId::from_uuid(Uuid::from_u128(1))),
            vec![Scope::DiscountsRead],
        ),
        ..World::new()
    };
    assert_eq!(
        w.create(Method::Codes, &[]),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::DiscountsWrite
        )))
    );
}

#[test]
fn a_code_another_discount_has_is_taken() {
    let w = World::new();
    w.create(Method::Codes, &["SUMMER"]).unwrap();
    assert_eq!(
        w.create(Method::Codes, &[" summer"]),
        Err(CommandError::CodeTaken("summer".to_string()))
    );
}

#[test]
fn a_code_listed_twice_is_taken() {
    let w = World::new();
    assert_eq!(
        w.create(Method::Codes, &["A1", "a1"]),
        Err(CommandError::CodeTaken("a1".to_string()))
    );
}

#[test]
fn an_automatic_discount_has_no_codes() {
    let w = World::new();
    assert!(matches!(
        w.create(Method::Automatic, &["X"]),
        Err(CommandError::AutomaticHasNoCodes(_))
    ));
}

#[test]
fn an_amount_in_the_wrong_currency_is_refused() {
    let w = World::new();
    let input = CreateDiscount {
        draft: DiscountDraft::new(
            "Off",
            Method::Automatic,
            Kind::Standard {
                rewards: vec![Reward {
                    target: Target::Subtotal,
                    value: Value::Amount {
                        amounts: PerMarket::of(mkt(), Money::new(500, Currency::USD).unwrap()),
                        allocation: hypha_discounts::Allocation::Across,
                    },
                }],
            },
        ),
        codes: Vec::new(),
        currencies: World::currencies(),
    };
    assert!(matches!(
        block_on(create_discount(&w.ctx, &w.ports, &w.ids, input)),
        Err(CommandError::Discount(
            DiscountError::CurrencyMismatch { .. }
        ))
    ));
}

#[test]
fn one_automatic_discount_past_the_most_live_is_refused() {
    let w = World::new();
    for _ in 0..MAX_LIVE_AUTOMATIC {
        w.create(Method::Automatic, &[]).unwrap();
    }
    assert_eq!(
        w.create(Method::Automatic, &[]),
        Err(CommandError::TooManyAutomatic)
    );
    // Discounts by code don't count.
    assert!(w.create(Method::Codes, &[]).is_ok());
}

#[test]
fn a_draft_doesnt_count_toward_the_most_live() {
    let w = World::new();
    for _ in 0..MAX_LIVE_AUTOMATIC {
        w.create(Method::Automatic, &[]).unwrap();
    }
    let mut draft = DiscountDraft::new("Later", Method::Automatic, percent_off_order());
    draft.status = Status::Draft;
    let input = CreateDiscount {
        draft,
        codes: Vec::new(),
        currencies: World::currencies(),
    };
    let created = block_on(create_discount(&w.ctx, &w.ports, &w.ids, input)).unwrap();
    // Making it active is one too many.
    let patch = DiscountPatch {
        status: Some(Status::Active),
        ..DiscountPatch::default()
    };
    assert_eq!(
        block_on(update_discount(
            &w.ctx,
            &w.ports,
            created.id(),
            patch,
            &World::currencies()
        )),
        Err(CommandError::TooManyAutomatic)
    );
}

#[test]
fn an_update_changes_only_what_it_names() {
    let w = World::new();
    let id = w.create(Method::Codes, &[]).unwrap();
    let patch = DiscountPatch {
        title: Some("Renamed".to_string()),
        ends_at: Some(Some(Timestamp::from_unix_micros(i64::MAX / 2))),
        ..DiscountPatch::default()
    };
    let updated = block_on(update_discount(
        &w.ctx,
        &w.ports,
        id,
        patch,
        &World::currencies(),
    ))
    .unwrap();
    assert_eq!(updated.title(), "Renamed");
    assert_eq!(updated.kind(), &percent_off_order());
    assert_eq!(updated.version().value(), 2);
    assert_eq!(
        w.events("discount.updated"),
        vec![serde_json::json!({"discount": id, "fields": ["ends_at", "title"]})]
    );
}

#[test]
fn an_unchanged_update_writes_nothing() {
    let w = World::new();
    let id = w.create(Method::Codes, &[]).unwrap();
    let updated = block_on(update_discount(
        &w.ctx,
        &w.ports,
        id,
        DiscountPatch::default(),
        &World::currencies(),
    ))
    .unwrap();
    assert_eq!(updated.version().value(), 1);
}

#[test]
fn deleting_takes_its_codes() {
    let w = World::new();
    let id = w.create(Method::Codes, &["BYE"]).unwrap();
    block_on(delete_discount(&w.ctx, &w.ports, id)).unwrap();
    assert_eq!(
        block_on(get_discount(&w.ctx, &w.ports, id)),
        Err(CommandError::NotFound(id))
    );
    assert_eq!(
        block_on(w.ports.discounts.find_codes(&["BYE".to_string()])),
        Ok(vec![])
    );
    assert_eq!(w.events("discount.deleted").len(), 1);
}

#[test]
fn codes_are_added_all_or_none() {
    let w = World::new();
    let id = w.create(Method::Codes, &["OLD"]).unwrap();
    let codes = |list: &[&str]| {
        list.iter()
            .map(|c| NewCode {
                code: c.to_string(),
                usage_limit: Some(1),
            })
            .collect()
    };
    assert_eq!(
        block_on(add_codes(
            &w.ctx,
            &w.ports,
            id,
            codes(&["NEW", "old"]),
            None
        )),
        Err(CommandError::CodeTaken("old".to_string()))
    );
    assert_eq!(w.codes_of(id), vec!["OLD"]);
    block_on(add_codes(
        &w.ctx,
        &w.ports,
        id,
        codes(&["NEW"]),
        Some("Influencers".to_string()),
    ))
    .unwrap();
    assert_eq!(w.codes_of(id), vec!["NEW", "OLD"]);
}

#[test]
fn generated_codes_are_drawn_again_when_taken() {
    let w = World::new();
    let env = TestEnv::default();
    let id = w.create(Method::Codes, &[]).unwrap();
    let generation = Generation {
        count: 50,
        prefix: Some("nl-".to_string()),
        length: 11,
    };
    let codes = block_on(generate_codes(
        &w.ctx,
        &w.ports,
        &env,
        id,
        generation,
        Some(1),
        Some("Newsletter".to_string()),
    ))
    .unwrap();
    assert_eq!(codes.len(), 50);
    assert!(codes.iter().all(|c| c.normalized.starts_with("NL-")
        && c.normalized.len() == 11
        && c.usage_limit == Some(1)));
    assert_eq!(
        w.events("discount.codes_added"),
        vec![serde_json::json!({"discount": id, "count": 50, "batch": "Newsletter"})]
    );
}

#[test]
fn generating_beyond_the_most_at_once_is_refused() {
    let w = World::new();
    let id = w.create(Method::Codes, &[]).unwrap();
    let generation = Generation {
        count: 10_001,
        prefix: None,
        length: 12,
    };
    assert_eq!(
        block_on(generate_codes(
            &w.ctx,
            &w.ports,
            &TestEnv::default(),
            id,
            generation,
            None,
            None
        )),
        Err(CommandError::Code(CodeError::TooMany))
    );
}

#[test]
fn a_batch_of_codes_is_removed_together() {
    let w = World::new();
    let id = w.create(Method::Codes, &["KEEP"]).unwrap();
    let codes = vec![
        NewCode {
            code: "A".to_string(),
            usage_limit: None,
        },
        NewCode {
            code: "B".to_string(),
            usage_limit: None,
        },
    ];
    block_on(add_codes(
        &w.ctx,
        &w.ports,
        id,
        codes,
        Some("Spring".to_string()),
    ))
    .unwrap();
    let removed = block_on(remove_codes(
        &w.ctx,
        &w.ports,
        id,
        CodeSelection::Batch("Spring".to_string()),
    ))
    .unwrap();
    assert_eq!(removed, 2);
    assert_eq!(w.codes_of(id), vec!["KEEP"]);
    assert_eq!(
        w.events("discount.codes_removed"),
        vec![serde_json::json!({"discount": id, "count": 2, "batch": "Spring"})]
    );
}

#[test]
fn listed_codes_are_removed_by_their_normalised_text() {
    let w = World::new();
    let id = w.create(Method::Codes, &["KEEP", "DROP"]).unwrap();
    let removed = block_on(remove_codes(
        &w.ctx,
        &w.ports,
        id,
        CodeSelection::Codes(vec![" drop ".to_string()]),
    ))
    .unwrap();
    assert_eq!(removed, 1);
    assert_eq!(w.codes_of(id), vec!["KEEP"]);
}

#[test]
fn the_list_finds_a_discount_by_its_code_however_typed() {
    let w = World::new();
    let id = w.create(Method::Codes, &["WELCOME10"]).unwrap();
    w.create(Method::Codes, &["OTHER"]).unwrap();
    let mut filter = DiscountFilter::everything(Timestamp::UNIX_EPOCH);
    filter.code = Some(" welcome10".to_string());
    let found = block_on(list_discounts(&w.ctx, &w.ports, filter, page())).unwrap();
    assert_eq!(
        found.items.iter().map(|d| d.id()).collect::<Vec<_>>(),
        vec![id]
    );
}

#[test]
fn a_live_code_is_found_and_others_read_as_none() {
    let w = World::new();
    w.create(Method::Codes, &["LIVE"]).unwrap();
    let now = Timestamp::UNIX_EPOCH;
    assert!(
        block_on(find_live_code(&w.ports, "LIVE", now))
            .unwrap()
            .is_some()
    );
    assert_eq!(block_on(find_live_code(&w.ports, "NOPE", now)), Ok(None));
}
