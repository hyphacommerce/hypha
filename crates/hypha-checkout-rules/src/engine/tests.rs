use std::collections::HashSet;

use hypha_core::{AddressParts, Currency, Severity};
use uuid::Uuid;

use super::*;
use crate::rule::{PostalRange, RuleAppliesTo, RuleDraft, RuleOutcome};

fn rule_id(n: u128) -> hypha_core::CheckoutRuleId {
    hypha_core::CheckoutRuleId::from_uuid(Uuid::from_u128(n))
}

fn option(n: u128) -> ShippingOptionId {
    ShippingOptionId::from_uuid(Uuid::from_u128(n))
}

fn product(n: u128) -> ProductId {
    ProductId::from_uuid(Uuid::from_u128(n))
}

fn category(n: u128) -> CategoryId {
    CategoryId::from_uuid(Uuid::from_u128(n))
}

fn market(n: u128) -> MarketId {
    MarketId::from_uuid(Uuid::from_u128(n))
}

fn eur(amount: i64) -> Money {
    Money::new(amount, Currency::EUR).unwrap()
}

fn address(country: &str, line1: &str) -> AddressParts {
    AddressParts {
        first_name: None,
        last_name: "Horvat".to_string(),
        company: None,
        line1: line1.to_string(),
        line2: None,
        city: "Zagreb".to_string(),
        province: None,
        postal_code: None,
        country: country.to_string(),
        phone: None,
    }
}

fn input() -> RuleInput {
    RuleInput {
        address: Some(address("HR", "Ilica 1")),
        flags: AddressFlags::default(),
        verification: None,
        email: Some("ana@example.com".to_string()),
        country: CountryCode::new("HR").unwrap(),
        market: Some(market(1)),
        channel: None,
        shipping: vec![None],
        lines: vec![RuleLine {
            product: product(1),
            categories: HashSet::new(),
            quantity: 2,
            shipping_weight: Some(Weight::from_grams(300)),
        }],
        subtotal: Some(eur(5000)),
    }
}

fn rule_with(n: u128, applies_to: RuleAppliesTo, conditions: Vec<RuleCondition>) -> CheckoutRule {
    CheckoutRule::new(
        rule_id(n),
        u32::try_from(n).unwrap(),
        RuleDraft {
            name: format!("Rule {n}"),
            enabled: true,
            applies_to,
            conditions,
            outcome: RuleOutcome {
                severity: Severity::Error,
                message: format!("Rule {n} says no."),
                path: None,
                code: None,
            },
        },
    )
    .unwrap()
}

fn rule(conditions: Vec<RuleCondition>) -> CheckoutRule {
    rule_with(1, RuleAppliesTo::default(), conditions)
}

fn fires(input: &RuleInput, condition: RuleCondition) -> bool {
    !evaluate(input, &[rule(vec![condition])]).is_empty()
}

fn paths(input: &RuleInput, rules: &[CheckoutRule]) -> Vec<String> {
    evaluate(input, rules)
        .iter()
        .map(|m| m.path().to_string())
        .collect()
}

fn missing(field: RuleField) -> RuleCondition {
    RuleCondition::FieldMissing { field }
}

fn flag(flag: AddressFlag) -> RuleCondition {
    RuleCondition::AddressIs { flag }
}

fn postal_in(prefixes: &[&str], ranges: &[(&str, &str)], codes: &[&str]) -> RuleCondition {
    let texts = |list: &[&str]| list.iter().map(|s| s.to_string()).collect();
    RuleCondition::PostalCodeIn {
        prefixes: texts(prefixes),
        ranges: ranges
            .iter()
            .map(|(from, to)| PostalRange {
                from: from.to_string(),
                to: to.to_string(),
            })
            .collect(),
        codes: texts(codes),
    }
}

fn with_postal(code: &str) -> RuleInput {
    let mut i = input();
    if let Some(a) = i.address.as_mut() {
        a.country = "ES".to_string();
        a.postal_code = Some(code.to_string());
    }
    i
}

#[test]
fn a_rule_whose_conditions_hold_gives_its_message() {
    let mut i = input();
    i.flags.po_box = true;
    let messages = evaluate(&i, &[rule(vec![flag(AddressFlag::PoBox)])]);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].message(), "Rule 1 says no.");
    assert_eq!(messages[0].severity(), Severity::Error);
    assert_eq!(messages[0].code().as_str(), "rule");
    assert_eq!(
        messages[0].source(),
        &MessageSource::Rule { rule: rule_id(1) }
    );
}

#[test]
fn every_condition_must_hold() {
    let mut i = input();
    i.flags.po_box = true;
    let both = rule(vec![flag(AddressFlag::PoBox), missing(RuleField::Phone)]);
    assert_eq!(evaluate(&i, std::slice::from_ref(&both)).len(), 1);
    if let Some(a) = i.address.as_mut() {
        a.phone = Some("+385 1 234 5678".to_string());
    }
    assert!(evaluate(&i, &[both]).is_empty());
}

#[test]
fn a_disabled_rule_says_nothing() {
    let mut r = rule(vec![missing(RuleField::Phone)]);
    let mut draft = r.draft().clone();
    draft.enabled = false;
    r.update(draft).unwrap();
    assert!(evaluate(&input(), &[r]).is_empty());
}

#[test]
fn a_rule_about_the_address_waits_for_one() {
    let mut i = input();
    i.address = None;
    assert!(!fires(&i, missing(RuleField::Phone)));
    assert!(!fires(
        &i,
        RuleCondition::AddressIsNot {
            flag: AddressFlag::PoBox
        }
    ));
}

#[test]
fn a_rule_about_the_email_needs_no_address() {
    let mut i = input();
    i.address = None;
    i.email = None;
    assert!(fires(&i, missing(RuleField::Email)));
}

#[test]
fn messages_come_in_the_rules_order() {
    let rules = [
        rule_with(1, RuleAppliesTo::default(), vec![missing(RuleField::Phone)]),
        rule_with(
            2,
            RuleAppliesTo::default(),
            vec![missing(RuleField::Company)],
        ),
    ];
    assert_eq!(
        paths(&input(), &rules),
        ["$.shipping_address.phone", "$.shipping_address.company"]
    );
}

#[test]
fn included_countries_are_the_addresss() {
    let applies = |codes: RuleCountries| RuleAppliesTo {
        countries: codes,
        ..RuleAppliesTo::default()
    };
    let hr = vec![CountryCode::new("HR").unwrap()];
    let r = rule_with(
        1,
        applies(RuleCountries::Include { codes: hr.clone() }),
        vec![missing(RuleField::Phone)],
    );
    assert_eq!(evaluate(&input(), std::slice::from_ref(&r)).len(), 1);
    let mut abroad = input();
    if let Some(a) = abroad.address.as_mut() {
        a.country = "SI".to_string();
    }
    assert!(evaluate(&abroad, &[r]).is_empty());
    let r = rule_with(
        1,
        applies(RuleCountries::Exclude { codes: hr }),
        vec![missing(RuleField::Phone)],
    );
    assert!(evaluate(&input(), std::slice::from_ref(&r)).is_empty());
    assert_eq!(evaluate(&abroad, &[r]).len(), 1);
}

#[test]
fn a_rule_for_markets_needs_one_of_them() {
    let applies = RuleAppliesTo {
        markets: vec![market(2)],
        ..RuleAppliesTo::default()
    };
    let r = rule_with(1, applies, vec![missing(RuleField::Phone)]);
    assert!(evaluate(&input(), std::slice::from_ref(&r)).is_empty());
    let mut i = input();
    i.market = Some(market(2));
    assert_eq!(evaluate(&i, std::slice::from_ref(&r)).len(), 1);
    i.market = None;
    assert!(evaluate(&i, &[r]).is_empty());
}

#[test]
fn a_rule_for_options_waits_for_one_and_points_at_its_group() {
    let applies = RuleAppliesTo {
        shipping_options: vec![option(7)],
        ..RuleAppliesTo::default()
    };
    let r = rule_with(1, applies, vec![missing(RuleField::Phone)]);
    let mut i = input();
    i.shipping = vec![Some(option(3)), None];
    assert!(evaluate(&i, std::slice::from_ref(&r)).is_empty());
    i.shipping = vec![Some(option(3)), Some(option(7))];
    assert_eq!(paths(&i, &[r]), ["$.shipping_groups[1]"]);
}

#[test]
fn the_merchants_path_wins() {
    let mut r = rule(vec![missing(RuleField::Phone)]);
    let mut draft = r.draft().clone();
    draft.outcome.path = Some("$".to_string());
    r.update(draft).unwrap();
    assert_eq!(paths(&input(), &[r]), ["$"]);
}

#[test]
fn a_pattern_matches_with_the_case_the_merchant_chose() {
    let pattern = |case_insensitive| RuleCondition::FieldMatches {
        field: RuleField::Line1,
        pattern: r"^ilica\b".to_string(),
        case_insensitive,
    };
    assert!(!fires(&input(), pattern(false)));
    assert!(fires(&input(), pattern(true)));
}

#[test]
fn an_empty_field_reads_as_empty_text() {
    let not_digits = RuleCondition::FieldNotMatches {
        field: RuleField::Line2,
        pattern: r"\d".to_string(),
        case_insensitive: false,
    };
    assert!(fires(&input(), not_digits));
}

#[test]
fn contains_ignores_case() {
    let condition = RuleCondition::FieldContains {
        field: RuleField::Line1,
        values: vec!["ILICA".to_string()],
    };
    assert!(fires(&input(), condition));
}

#[test]
fn length_counts_characters_not_bytes() {
    let mut i = input();
    if let Some(a) = i.address.as_mut() {
        a.line1 = "Šćž 1".to_string();
    }
    let longer = |length| RuleCondition::FieldLongerThan {
        field: RuleField::Line1,
        length,
    };
    assert!(fires(&i, longer(4)));
    assert!(!fires(&i, longer(5)));
}

#[test]
fn a_postal_prefix_ignores_spaces_and_the_country_prefix() {
    assert!(fires(
        &with_postal("ES-35 001"),
        postal_in(&["35"], &[], &[])
    ));
    assert!(!fires(&with_postal("28039"), postal_in(&["35"], &[], &[])));
}

#[test]
fn a_postal_range_compares_codes_of_its_length() {
    let range = || postal_in(&[], &[("35000", "38999")], &[]);
    assert!(fires(&with_postal("35000"), range()));
    assert!(fires(&with_postal("38999"), range()));
    assert!(!fires(&with_postal("39000"), range()));
    assert!(!fires(&with_postal("3600"), range()));
}

#[test]
fn a_postal_code_listed_whole_matches_only_itself() {
    assert!(fires(
        &with_postal("1016 gv"),
        postal_in(&[], &[], &["1016GV"])
    ));
    assert!(!fires(
        &with_postal("1016 GW"),
        postal_in(&[], &[], &["1016GV"])
    ));
}

#[test]
fn no_postal_code_is_in_no_list() {
    let not_in = RuleCondition::PostalCodeNotIn {
        prefixes: vec!["35".to_string()],
        ranges: Vec::new(),
        codes: Vec::new(),
    };
    assert!(fires(&input(), not_in));
    assert!(!fires(&input(), postal_in(&["35"], &[], &[])));
}

#[test]
fn a_region_compares_its_code_ignoring_case() {
    let mut i = input();
    if let Some(a) = i.address.as_mut() {
        a.province = Some("ca".to_string());
    }
    let condition = RuleCondition::RegionIn {
        regions: vec!["CA".to_string()],
    };
    assert!(fires(&i, condition));
}

#[test]
fn no_kept_verdict_is_no_verdict() {
    let condition = || RuleCondition::VerificationIs {
        verdicts: vec![AddressVerdict::Unknown],
    };
    assert!(!fires(&input(), condition()));
    let mut i = input();
    i.verification = Some(VerifiedFacts {
        verdict: Some(AddressVerdict::Unknown),
        ..VerifiedFacts::default()
    });
    assert!(fires(&i, condition()));
}

#[test]
fn an_unknown_flag_counts_as_not() {
    assert!(!fires(&input(), flag(AddressFlag::Residential)));
    assert!(fires(
        &input(),
        RuleCondition::AddressIsNot {
            flag: AddressFlag::Residential
        }
    ));
}

#[test]
fn verification_can_find_a_po_box_the_text_doesnt_show() {
    let mut i = input();
    i.verification = Some(VerifiedFacts {
        po_box: Some(true),
        ..VerifiedFacts::default()
    });
    assert!(fires(&i, flag(AddressFlag::PoBox)));
}

#[test]
fn a_cart_condition_finds_a_line_by_category_and_points_at_it() {
    let mut i = input();
    i.lines.push(RuleLine {
        product: product(2),
        categories: HashSet::from([category(5), category(9)]),
        quantity: 1,
        shipping_weight: None,
    });
    let contains = RuleCondition::CartContains {
        products: Vec::new(),
        categories: vec![category(9)],
    };
    assert_eq!(paths(&i, &[rule(vec![contains])]), ["$.lines[1]"]);
}

#[test]
fn a_subtotal_in_a_currency_not_listed_doesnt_hold() {
    let at_least = |amount| RuleCondition::SubtotalAtLeast {
        amounts: vec![amount],
    };
    let below = |amount| RuleCondition::SubtotalBelow {
        amounts: vec![amount],
    };
    assert!(fires(&input(), at_least(eur(5000))));
    assert!(!fires(&input(), below(eur(5000))));
    let usd = Money::new(1, Currency::USD).unwrap();
    assert!(!fires(&input(), at_least(usd)));
    assert!(!fires(&input(), below(usd)));
}

#[test]
fn weight_counts_what_is_shipped() {
    let mut i = input();
    i.lines.push(RuleLine {
        product: product(2),
        categories: HashSet::new(),
        quantity: 5,
        shipping_weight: None,
    });
    assert!(fires(&i, RuleCondition::WeightAtLeast { grams: 600 }));
    assert!(fires(&i, RuleCondition::WeightBelow { grams: 601 }));
    assert!(!fires(&i, RuleCondition::WeightBelow { grams: 600 }));
}

#[test]
fn a_flag_points_at_the_first_street_line() {
    let mut i = input();
    i.flags.military = true;
    assert_eq!(
        paths(&i, &[rule(vec![flag(AddressFlag::Military)])]),
        ["$.shipping_address.line1"]
    );
}

#[test]
fn rules_naming_categories_are_found() {
    assert!(!names_categories(&[rule(vec![missing(RuleField::Phone)])]));
    let named = rule(vec![RuleCondition::CartNotContains {
        products: Vec::new(),
        categories: vec![category(1)],
    }]);
    assert!(names_categories(&[named]));
}
