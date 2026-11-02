//! Checkout rules: conditions on a cart's shipping address and contents that a merchant
//! sets up without code, each ending in an error (which stops checkout) or a warning with
//! the merchant's message on one field. A leaf crate on core: checkout hands the engine
//! the cart as plain values.

pub mod commands;
pub mod engine;
pub mod error;
pub mod events;
pub mod repo;
pub mod rule;
pub mod settings;

pub use commands::{
    CommandError, RulePatch, UpdateCheckoutSettings, create_checkout_rule, delete_checkout_rule,
    get_checkout_rule, get_checkout_settings, list_checkout_rules, update_checkout_rule,
    update_checkout_settings,
};
pub use engine::{RuleInput, RuleLine, VerifiedFacts, evaluate, names_categories};
pub use error::{CheckoutRuleError, Problem};
pub use events::{CHECKOUT_RULE, CheckoutRuleCreated, CheckoutRuleDeleted, CheckoutRuleUpdated};
pub use repo::{
    CheckoutRuleRepo, CheckoutSettingsRepo, HasCheckoutRules, HasCheckoutSettings, keys,
};
pub use rule::{
    AddressFlag, CheckoutRule, CheckoutRuleSnapshot, MAX_AMOUNTS, MAX_CONDITIONS, MAX_FIELD_LENGTH,
    MAX_LIST, MAX_MESSAGE_CHARS, MAX_PATTERN_CHARS, MAX_POSTAL_ENTRIES, MAX_RULES, MAX_VALUES,
    PostalRange, RuleAppliesTo, RuleCondition, RuleCountries, RuleDraft, RuleField, RuleOutcome,
};
pub use settings::{
    AUTOCOMPLETE_POINTS, CHECKOUT_SETTINGS, CheckoutSettings, CheckoutSettingsSnapshot,
    CheckoutSettingsUpdated, VERIFY_POINT,
};
