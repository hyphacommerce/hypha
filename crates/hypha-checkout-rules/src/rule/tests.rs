use hypha_core::{Recorded, Severity};
use uuid::Uuid;

use super::*;

fn id() -> CheckoutRuleId {
    CheckoutRuleId::from_uuid(Uuid::from_u128(1))
}

fn po_box() -> RuleCondition {
    RuleCondition::AddressIs {
        flag: AddressFlag::PoBox,
    }
}

fn draft() -> RuleDraft {
    RuleDraft {
        name: "No PO boxes".to_string(),
        enabled: true,
        applies_to: RuleAppliesTo::default(),
        conditions: vec![po_box()],
        outcome: RuleOutcome {
            severity: Severity::Error,
            message: "We can't ship to PO boxes.".to_string(),
            path: None,
            code: None,
        },
    }
}

fn with(condition: RuleCondition) -> RuleDraft {
    RuleDraft {
        conditions: vec![condition],
        ..draft()
    }
}

fn problems(draft: RuleDraft) -> Vec<(String, &'static str)> {
    match CheckoutRule::new(id(), 0, draft) {
        Err(CheckoutRuleError::Invalid(problems)) => {
            problems.into_iter().map(|p| (p.path, p.code)).collect()
        }
        Ok(_) => Vec::new(),
    }
}

fn problem(path: &str, code: &'static str) -> Vec<(String, &'static str)> {
    vec![(path.to_string(), code)]
}

fn matches(pattern: &str) -> RuleCondition {
    RuleCondition::FieldMatches {
        field: RuleField::Line1,
        pattern: pattern.to_string(),
        case_insensitive: false,
    }
}

fn postal(ranges: &[(&str, &str)]) -> RuleCondition {
    RuleCondition::PostalCodeIn {
        prefixes: Vec::new(),
        ranges: ranges
            .iter()
            .map(|(from, to)| PostalRange {
                from: from.to_string(),
                to: to.to_string(),
            })
            .collect(),
        codes: Vec::new(),
    }
}

#[test]
fn a_new_rule_records_created() {
    let mut rule = CheckoutRule::new(id(), 0, draft()).unwrap();
    let events = rule.take_events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_type, "checkout_rule.created");
    assert!(rule.take_events().is_empty());
}

#[test]
fn the_name_is_trimmed_and_required() {
    let rule = CheckoutRule::new(
        id(),
        0,
        RuleDraft {
            name: "  No PO boxes ".to_string(),
            ..draft()
        },
    )
    .unwrap();
    assert_eq!(rule.name(), "No PO boxes");
    assert_eq!(
        problems(RuleDraft {
            name: " ".to_string(),
            ..draft()
        }),
        problem("name", "invalid_text")
    );
}

#[test]
fn every_problem_is_reported_at_once() {
    let mut d = draft();
    d.conditions.clear();
    d.outcome.message = String::new();
    d.outcome.code = Some("Not A Code".to_string());
    assert_eq!(
        problems(d),
        vec![
            ("conditions".to_string(), "empty"),
            ("outcome.message".to_string(), "empty"),
            ("outcome.code".to_string(), "invalid_code"),
        ]
    );
}

#[test]
fn at_most_ten_conditions() {
    let mut d = draft();
    d.conditions = vec![po_box(); 10];
    assert!(CheckoutRule::new(id(), 0, d.clone()).is_ok());
    d.conditions.push(po_box());
    assert_eq!(problems(d), problem("conditions", "too_many"));
}

#[test]
fn the_message_is_at_most_300_characters() {
    let mut d = draft();
    d.outcome.message = "é".repeat(300);
    assert!(CheckoutRule::new(id(), 0, d.clone()).is_ok());
    d.outcome.message.push('é');
    assert_eq!(problems(d), problem("outcome.message", "too_long"));
}

#[test]
fn a_pattern_that_does_not_compile_is_refused_where_it_is() {
    assert_eq!(
        problems(with(matches("(unclosed"))),
        problem("conditions[0].pattern", "invalid_pattern")
    );
}

#[test]
fn a_pattern_is_at_most_300_characters() {
    assert!(CheckoutRule::new(id(), 0, with(matches(&"a".repeat(300)))).is_ok());
    assert_eq!(
        problems(with(matches(&"a".repeat(301)))),
        problem("conditions[0].pattern", "too_long")
    );
}

#[test]
fn a_message_may_not_point_at_a_line() {
    let mut d = draft();
    d.outcome.path = Some("$.lines[0]".to_string());
    assert_eq!(problems(d), problem("outcome.path", "path_not_allowed"));
}

#[test]
fn a_message_points_at_a_cart_path() {
    let mut d = draft();
    d.outcome.path = Some("$.shipping_address.nickname".to_string());
    assert_eq!(problems(d.clone()), problem("outcome.path", "invalid_path"));
    d.outcome.path = Some(" $.shipping_address.line2 ".to_string());
    let rule = CheckoutRule::new(id(), 0, d).unwrap();
    assert_eq!(
        rule.path(),
        Some(MessagePath::ShippingAddressField(AddressField::Line2))
    );
    assert_eq!(
        rule.outcome().path.as_deref(),
        Some("$.shipping_address.line2")
    );
}

#[test]
fn the_code_is_rule_unless_the_merchant_gives_one() {
    let rule = CheckoutRule::new(id(), 0, draft()).unwrap();
    assert_eq!(rule.code().as_str(), "rule");
    let mut d = draft();
    d.outcome.code = Some("po_box_not_allowed".to_string());
    let rule = CheckoutRule::new(id(), 0, d).unwrap();
    assert_eq!(rule.code().as_str(), "po_box_not_allowed");
}

#[test]
fn a_range_has_ends_of_one_length() {
    assert_eq!(
        problems(with(postal(&[("3500", "35999")]))),
        problem("conditions[0].ranges[0]", "lengths_differ")
    );
}

#[test]
fn a_range_starts_at_or_before_its_end() {
    assert!(CheckoutRule::new(id(), 0, with(postal(&[("35000", "35000")]))).is_ok());
    assert_eq!(
        problems(with(postal(&[("39999", "35000")]))),
        problem("conditions[0].ranges[0]", "not_ascending")
    );
}

#[test]
fn a_postal_condition_lists_something() {
    assert_eq!(
        problems(with(postal(&[]))),
        problem("conditions[0]", "empty")
    );
}

#[test]
fn values_listed_twice_ignoring_case_are_refused() {
    let condition = RuleCondition::FieldContains {
        field: RuleField::Line1,
        values: vec!["PO Box".to_string(), " po box ".to_string()],
    };
    assert_eq!(
        problems(with(condition)),
        problem("conditions[0].values[1]", "duplicate")
    );
}

#[test]
fn regions_are_kept_upper_cased() {
    let rule = CheckoutRule::new(
        id(),
        0,
        with(RuleCondition::RegionIn {
            regions: vec![" ca ".to_string()],
        }),
    )
    .unwrap();
    assert_eq!(
        rule.conditions(),
        [RuleCondition::RegionIn {
            regions: vec!["CA".to_string()]
        }]
    );
}

#[test]
fn a_cart_condition_names_a_product_or_category() {
    assert_eq!(
        problems(with(RuleCondition::CartContains {
            products: Vec::new(),
            categories: Vec::new(),
        })),
        problem("conditions[0]", "empty")
    );
}

#[test]
fn a_length_is_at_most_an_address_fields() {
    assert_eq!(
        problems(with(RuleCondition::FieldLongerThan {
            field: RuleField::Line1,
            length: 256,
        })),
        problem("conditions[0].length", "out_of_range")
    );
}

#[test]
fn an_included_country_list_names_one() {
    let mut d = draft();
    d.applies_to.countries = RuleCountries::Include { codes: Vec::new() };
    assert_eq!(problems(d), problem("applies_to.countries.codes", "empty"));
}

#[test]
fn an_update_names_what_changed() {
    let mut rule = CheckoutRule::new(id(), 0, draft()).unwrap();
    rule.take_events();
    let mut d = draft();
    d.enabled = false;
    d.outcome.severity = Severity::Warning;
    rule.update(d).unwrap();
    rule.set_position(3);
    let events = rule.take_events();
    assert_eq!(events[0].event_type, "checkout_rule.updated");
    assert_eq!(
        events[0].data["fields"],
        serde_json::json!(["enabled", "outcome", "position"])
    );
}

#[test]
fn the_same_draft_changes_nothing() {
    let mut rule = CheckoutRule::new(id(), 2, draft()).unwrap();
    rule.take_events();
    rule.update(draft()).unwrap();
    rule.set_position(2);
    assert!(!rule.has_changes());
}

#[test]
fn a_restored_rule_compiles_its_pattern_when_first_used() {
    let rule = CheckoutRule::new(id(), 0, with(matches("^Box"))).unwrap();
    let restored = CheckoutRule::restore(rule.snapshot()).unwrap();
    assert_eq!(restored, rule);
    assert!(restored.regex(0).is_some_and(|r| r.is_match("Box 12")));
}
