use hypha_core::{CheckoutRuleId, RepoError};

use crate::{rule::CheckoutRule, settings::CheckoutSettings};

/// Uniqueness rules the checkout rules repo enforces, reported as
/// `RepoError::Duplicate(key)`.
pub mod keys {
    pub const CHECKOUT_RULE_ID: &str = "checkout_rule_id";
    /// There's one settings record per store.
    pub const CHECKOUT_SETTINGS: &str = "checkout_settings";
}

/// The store's checkout rules.
#[allow(async_fn_in_trait)]
pub trait CheckoutRuleRepo {
    async fn get(&self, id: CheckoutRuleId) -> Result<Option<CheckoutRule>, RepoError>;
    /// Every rule, by position, then by ID: what every read of a cart evaluates, so it's
    /// one read of at most `MAX_RULES`.
    async fn list(&self) -> Result<Vec<CheckoutRule>, RepoError>;
    /// Stores a new rule (version 1) with its events. Enforces `CHECKOUT_RULE_ID`.
    async fn insert(&self, rule: &mut CheckoutRule) -> Result<(), RepoError>;
    /// Stores the rules' changes with their events in one commit, bumping each one's
    /// version, or none of them when any is stale (`StaleVersion`): moving a rule
    /// renumbers the others with it.
    async fn update_many(&self, rules: &mut [CheckoutRule]) -> Result<(), RepoError>;
    /// Removes it, writing its events, when still at the version loaded; otherwise
    /// `StaleVersion`.
    async fn delete(&self, rule: &mut CheckoutRule) -> Result<(), RepoError>;
}

pub trait HasCheckoutRules {
    type CheckoutRules: CheckoutRuleRepo;
    fn checkout_rules(&self) -> &Self::CheckoutRules;
}

impl<T: HasCheckoutRules + ?Sized> HasCheckoutRules for &T {
    type CheckoutRules = T::CheckoutRules;
    fn checkout_rules(&self) -> &Self::CheckoutRules {
        (**self).checkout_rules()
    }
}

/// The store's one settings record.
#[allow(async_fn_in_trait)]
pub trait CheckoutSettingsRepo {
    /// `None` until the merchant first saves.
    async fn get(&self) -> Result<Option<CheckoutSettings>, RepoError>;
    /// Stores the record (version 1) with its events. Enforces `CHECKOUT_SETTINGS`: a
    /// second record is a `Duplicate`.
    async fn insert(&self, settings: &mut CheckoutSettings) -> Result<(), RepoError>;
    /// Stores its changes with their events when still at the version loaded; otherwise
    /// `StaleVersion`.
    async fn update(&self, settings: &mut CheckoutSettings) -> Result<(), RepoError>;
}

pub trait HasCheckoutSettings {
    type CheckoutSettings: CheckoutSettingsRepo;
    fn checkout_settings(&self) -> &Self::CheckoutSettings;
}

impl<T: HasCheckoutSettings + ?Sized> HasCheckoutSettings for &T {
    type CheckoutSettings = T::CheckoutSettings;
    fn checkout_settings(&self) -> &Self::CheckoutSettings {
        (**self).checkout_settings()
    }
}
