//! What happens to checkout rules, as events. Contract types: a reshaped field is a new
//! `VERSION`, never an edit.

use hypha_core::{CheckoutRuleId, Event};
use serde::Serialize;

pub const CHECKOUT_RULE: &str = "checkout_rule";

/// `checkout_rule.created`.
#[derive(Debug, Serialize)]
pub struct CheckoutRuleCreated {
    pub checkout_rule: CheckoutRuleId,
}

impl Event for CheckoutRuleCreated {
    const TYPE: &'static str = "checkout_rule.created";
    const VERSION: u16 = 1;
}

/// `checkout_rule.updated`: `fields` names what changed (`name`, `enabled`, `position`,
/// `applies_to`, `conditions`, `outcome`).
#[derive(Debug, Serialize)]
pub struct CheckoutRuleUpdated {
    pub checkout_rule: CheckoutRuleId,
    pub fields: Vec<&'static str>,
}

impl Event for CheckoutRuleUpdated {
    const TYPE: &'static str = "checkout_rule.updated";
    const VERSION: u16 = 1;
}

/// `checkout_rule.deleted`.
#[derive(Debug, Serialize)]
pub struct CheckoutRuleDeleted {
    pub checkout_rule: CheckoutRuleId,
}

impl Event for CheckoutRuleDeleted {
    const TYPE: &'static str = "checkout_rule.deleted";
    const VERSION: u16 = 1;
}
