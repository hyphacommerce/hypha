//! What every checkout rules store must do: rules with every kind of condition, kept in
//! their order, moved together.

use hypha_checkout_rules::{
    AddressFlag, CheckoutRule, CheckoutRuleRepo, CheckoutSettings, CheckoutSettingsRepo,
    HasCheckoutRules, HasCheckoutSettings, PostalRange, RuleAppliesTo, RuleCondition,
    RuleCountries, RuleDraft, RuleField, RuleOutcome, keys,
};
use hypha_core::{
    AddressVerdict, CategoryId, ChannelId, CheckoutRuleId, CountryCode, Currency, HasOutbox,
    MarketId, Money, OutboxRepo, ProductId, RepoError, Severity, ShippingOptionId, Stamped,
    Version, Versioned,
};
use uuid::Uuid;

use super::scenario;

fn rid(n: u128) -> CheckoutRuleId {
    CheckoutRuleId::from_uuid(Uuid::from_u128(n))
}

fn draft(name: &str) -> RuleDraft {
    RuleDraft {
        name: name.to_string(),
        enabled: true,
        applies_to: RuleAppliesTo::default(),
        conditions: vec![RuleCondition::AddressIs {
            flag: AddressFlag::PoBox,
        }],
        outcome: RuleOutcome {
            severity: Severity::Error,
            message: "We can't ship to PO boxes.".to_string(),
            path: None,
            code: None,
        },
    }
}

fn rule(n: u128, position: u32) -> CheckoutRule {
    CheckoutRule::new(rid(n), position, draft(&format!("Rule {n}"))).unwrap()
}

/// A rule with every kind of condition and every part of where it applies.
fn full_draft() -> RuleDraft {
    let id = |n| Uuid::from_u128(n);
    let field = RuleField::Line1;
    let texts = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let range = PostalRange {
        from: "35000".to_string(),
        to: "35999".to_string(),
    };
    let eur = Money::new(5000, Currency::EUR).unwrap();
    RuleDraft {
        name: "Everything".to_string(),
        enabled: false,
        applies_to: RuleAppliesTo {
            countries: RuleCountries::Exclude {
                codes: vec![CountryCode::new("ES").unwrap()],
            },
            markets: vec![MarketId::from_uuid(id(1))],
            channels: vec![ChannelId::from_uuid(id(2))],
            shipping_options: vec![ShippingOptionId::from_uuid(id(3))],
        },
        conditions: vec![
            RuleCondition::FieldMissing {
                field: RuleField::Phone,
            },
            RuleCondition::FieldMatches {
                field,
                pattern: r"(?i)\bbox\b".to_string(),
                case_insensitive: true,
            },
            RuleCondition::FieldNotContains {
                field: RuleField::Email,
                values: texts(&["@example.com"]),
            },
            RuleCondition::FieldShorterThan { field, length: 35 },
            RuleCondition::PostalCodeNotIn {
                prefixes: texts(&["38"]),
                ranges: vec![range],
                codes: texts(&["51001"]),
            },
            RuleCondition::RegionNotIn {
                regions: texts(&["CA", "NY"]),
            },
            RuleCondition::VerificationIs {
                verdicts: vec![AddressVerdict::Undeliverable, AddressVerdict::Unknown],
            },
            RuleCondition::CartContains {
                products: vec![ProductId::from_uuid(id(4))],
                categories: vec![CategoryId::from_uuid(id(5))],
            },
            RuleCondition::SubtotalBelow { amounts: vec![eur] },
            RuleCondition::WeightAtLeast { grams: 30_000 },
        ],
        outcome: RuleOutcome {
            severity: Severity::Warning,
            message: "Check this address.".to_string(),
            path: Some("$.shipping_address.phone".to_string()),
            code: Some("phone_required".to_string()),
        },
    }
}

/// Runs every checkout rule scenario, each on fresh ports from `make`.
pub async fn checkout_rule_repo<S>(make: impl AsyncFn() -> S)
where
    S: HasCheckoutRules + HasOutbox,
{
    scenario("checkout rule: every condition survives a round trip, at version 1, stamped");
    round_trip(&make().await).await;
    scenario("checkout rule: the list is by position, then by ID");
    lists(&make().await).await;
    scenario("checkout rule: rules move together; a stale one fails them all");
    updates_together(&make().await).await;
    scenario("checkout rule: delete writes its event; a stale copy fails");
    deletes(&make().await).await;
}

async fn round_trip<S: HasCheckoutRules + HasOutbox>(ports: &S) {
    let repo = ports.checkout_rules();
    let mut full = CheckoutRule::new(rid(1), 4, full_draft()).unwrap();
    repo.insert(&mut full).await.unwrap();
    assert_eq!(full.version(), Version::new(1));
    let stored = repo.get(rid(1)).await.unwrap().unwrap();
    assert_eq!(stored, full);
    assert_eq!(stored.draft(), full.draft());
    assert_eq!(stored.position(), 4);
    assert!(stored.stamps().is_some());
    assert_eq!(repo.get(rid(9)).await, Ok(None));
    assert_eq!(
        repo.insert(&mut rule(1, 0)).await,
        Err(RepoError::Duplicate(keys::CHECKOUT_RULE_ID))
    );
    let events = ports.outbox().events_after(0, 10).await.unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_type, "checkout_rule.created");
}

async fn lists<S: HasCheckoutRules>(ports: &S) {
    let repo = ports.checkout_rules();
    for (n, position) in [(3, 1), (1, 2), (2, 1), (4, 0)] {
        repo.insert(&mut rule(n, position)).await.unwrap();
    }
    let order: Vec<CheckoutRuleId> = repo
        .list()
        .await
        .unwrap()
        .iter()
        .map(CheckoutRule::id)
        .collect();
    assert_eq!(order, [rid(4), rid(2), rid(3), rid(1)]);
}

async fn updates_together<S: HasCheckoutRules + HasOutbox>(ports: &S) {
    let repo = ports.checkout_rules();
    let (mut a, mut b) = (rule(1, 0), rule(2, 1));
    repo.insert(&mut a).await.unwrap();
    repo.insert(&mut b).await.unwrap();
    let stale_b = b.clone();

    a.set_position(1);
    b.set_position(0);
    b.update(full_draft()).unwrap();
    let mut moved = [a, b];
    repo.update_many(&mut moved).await.unwrap();
    assert!(moved.iter().all(|r| r.version() == Version::new(2)));
    let [a, b] = moved;
    assert_eq!(repo.get(rid(1)).await, Ok(Some(a.clone())));
    assert_eq!(repo.get(rid(2)).await, Ok(Some(b)));

    // One stale rule: neither is written.
    let mut fresh_a = a.clone();
    fresh_a.set_position(5);
    let mut stale_b = stale_b;
    stale_b.set_position(6);
    let mut both = [fresh_a, stale_b];
    assert_eq!(
        repo.update_many(&mut both).await,
        Err(RepoError::StaleVersion)
    );
    assert_eq!(repo.get(rid(1)).await, Ok(Some(a)));

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
        [
            "checkout_rule.created",
            "checkout_rule.created",
            "checkout_rule.updated",
            "checkout_rule.updated",
        ]
    );
}

async fn deletes<S: HasCheckoutRules + HasOutbox>(ports: &S) {
    let repo = ports.checkout_rules();
    let mut r = rule(1, 0);
    repo.insert(&mut r).await.unwrap();
    let mut stale = r.clone();
    r.set_position(3);
    repo.update_many(std::slice::from_mut(&mut r))
        .await
        .unwrap();
    stale.mark_deleted();
    assert_eq!(repo.delete(&mut stale).await, Err(RepoError::StaleVersion));
    r.mark_deleted();
    repo.delete(&mut r).await.unwrap();
    assert_eq!(repo.get(rid(1)).await, Ok(None));
    assert_eq!(repo.list().await, Ok(Vec::new()));
    let last = ports.outbox().events_after(0, 100).await.unwrap();
    assert_eq!(
        last.last().map(|e| e.event_type.as_str()),
        Some("checkout_rule.deleted")
    );
}

/// Runs every `CheckoutSettingsRepo` scenario, each on fresh ports from `make`.
pub async fn checkout_settings_repo<S>(make: impl AsyncFn() -> S)
where
    S: HasCheckoutSettings + HasOutbox,
{
    scenario("checkout settings: none until stored; every field round trips with its event");
    settings_round_trip(&make().await).await;
    scenario("checkout settings: one record, updated from its version only");
    settings_versions(&make().await).await;
}

fn verifying() -> CheckoutSettings {
    let mut s = CheckoutSettings::default();
    s.set_verification_app(Some(hypha_core::AppKey::new("app_verify").unwrap()));
    s.set_autocomplete_app(Some(hypha_core::AppKey::new("app_places").unwrap()));
    s.set_validation_apps(vec![
        hypha_checkout_rules::ValidationAppSetting {
            app: hypha_core::AppKey::new("app_rules").unwrap(),
            enabled: false,
            block_on_failure: true,
        },
        hypha_checkout_rules::ValidationAppSetting::default_for(
            hypha_core::AppKey::new("app_alcohol").unwrap(),
        ),
    ]);
    s
}

async fn settings_round_trip<S: HasCheckoutSettings + HasOutbox>(ports: &S) {
    let repo = ports.checkout_settings();
    assert_eq!(repo.get().await, Ok(None));
    let mut s = verifying();
    repo.insert(&mut s).await.unwrap();
    assert_eq!(s.version(), Version::new(1));
    let stored = repo.get().await.unwrap().unwrap();
    assert_eq!(stored, s);
    assert!(stored.stamps().is_some());
    let events = ports.outbox().events_after(0, 10).await.unwrap();
    assert_eq!(
        events
            .iter()
            .map(|e| e.event_type.as_str())
            .collect::<Vec<_>>(),
        vec!["checkout_settings.updated"]
    );
}

async fn settings_versions<S: HasCheckoutSettings>(ports: &S) {
    let repo = ports.checkout_settings();
    let mut s = verifying();
    repo.insert(&mut s).await.unwrap();
    let mut again = CheckoutSettings::default();
    assert_eq!(
        repo.insert(&mut again).await,
        Err(RepoError::Duplicate(keys::CHECKOUT_SETTINGS))
    );
    let mut stale = repo.get().await.unwrap().unwrap();
    s.set_verification_app(None);
    s.set_autocomplete_app(None);
    s.set_validation_apps(Vec::new());
    repo.update(&mut s).await.unwrap();
    assert_eq!(s.version(), Version::new(2));
    assert_eq!(repo.get().await, Ok(Some(s.clone())));
    stale.set_verification_app(Some(hypha_core::AppKey::new("app_other").unwrap()));
    assert_eq!(repo.update(&mut stale).await, Err(RepoError::StaleVersion));
}
