//! Stripe, built into Hypha's API: card payments (and every other Payment Element method)
//! with no service of its own to host. Ported from the first-party Stripe app
//! (`hypha-app-stripe`), which stays as the example of an external payment app; this is the
//! recommended way to take cards.
//!
//! - **The provider** `spore_stripe` (`provider.rs`): starting a payment creates a
//!   PaymentIntent (idempotency key: Hypha's payment ID) and answers the storefront's
//!   `<stripe-payment>` element with its client secret; capture, void and refund call
//!   Stripe, its "no" becoming a refusal staff see.
//! - **Stripe's webhook** `POST /spores/stripe/webhook` (`webhook.rs`): verified with
//!   the signing secret, each event reported in-process with
//!   `record_payment_event`, its ID the idempotency key, so a redelivery changes nothing.
//! - **An order cancelled** cancels its open PaymentIntents (`cancel.rs`).
//! - **The admin**: an `order.payment` block (Stripe's status, amounts, a dashboard
//!   link) whose Capture, Void and Refund buttons run through the payment's own routes
//!   (`ui.rs`), and the settings form from the schema below.
//! - **The storefront**: `<stripe-payment>`, served from `GET
//!   /spores/stripe/elements/payment.js` (`element.rs`, the app's bundle as is).
//!
//! It keeps no tables: the payment's `reference` is the PaymentIntent's ID, the intent's
//! metadata names the Hypha payment, and core's idempotency keys dedupe Stripe's events.

pub mod amounts;
pub mod api;
pub mod cancel;
pub mod element;
pub mod provider;
pub mod signature;
pub mod ui;
pub mod webhook;

use hypha_spore::core::Scope;
use hypha_spore::migrations::MigrationSet;
use hypha_spore::settings::SettingsProblem;
use hypha_spore::{Spore, SporeEnv, SporeError};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::cancel::CancelIntents;
use crate::provider::StripeProvider;

/// The spore.
pub struct Stripe {
    pub env: SporeEnv,
    /// Stripe's API, or a fake (`[spores.stripe] api_base`).
    pub api_base: String,
}

/// Its section of `hypha.config.toml`, `[spores.stripe]`: the operator's.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    /// Stripe's API base (`https://api.stripe.com`); tests point it at a fake.
    #[serde(default)]
    api_base: Option<String>,
}

/// How a payment is collected.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureMode {
    /// Charged when the shopper confirms.
    #[default]
    Automatic,
    /// Authorized when the shopper confirms; staff capture it (within Stripe's window).
    Manual,
}

impl CaptureMode {
    /// The mode it isn't.
    pub fn other(self) -> Self {
        match self {
            CaptureMode::Automatic => CaptureMode::Manual,
            CaptureMode::Manual => CaptureMode::Automatic,
        }
    }
}

/// The merchant's settings, as the spore reads them: secrets opened. Every field is
/// optional until the merchant saves them.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub secret_key: Option<String>,
    #[serde(default)]
    pub publishable_key: Option<String>,
    #[serde(default)]
    pub webhook_secret: Option<String>,
    #[serde(default)]
    pub capture_mode: CaptureMode,
}

impl Settings {
    /// Read from a provider's `store_settings` (the host fills them).
    pub fn from_map(map: &Map<String, Value>) -> Self {
        serde_json::from_value(Value::Object(map.clone())).unwrap_or_default()
    }

    /// Whether the keys are test keys: the dashboard's test pages.
    pub fn is_live(&self) -> bool {
        self.secret_key
            .as_deref()
            .is_some_and(|k| k.contains("_live_"))
    }
}

impl Spore for Stripe {
    const NAME: &'static str = "stripe";
    /// Payments to start, act on and report; orders read for the block; the buyer's
    /// email for Stripe's receipt.
    const SCOPES: &'static [Scope] = &[
        Scope::OrdersRead,
        Scope::PaymentsWrite,
        Scope::CustomerDataEmail,
    ];

    fn title() -> &'static str {
        "Stripe"
    }

    fn migrations() -> MigrationSet {
        MigrationSet {
            module: "spore_stripe",
            migrations: &[],
        }
    }

    fn new(env: SporeEnv) -> Result<Self, SporeError> {
        let config: Config = env.config_as()?;
        Ok(Stripe {
            api_base: config.api_base.unwrap_or_else(|| api::API_BASE.to_string()),
            env,
        })
    }

    fn settings_schema() -> Option<Value> {
        Some(json!({
            "type": "object",
            "title": "Stripe",
            "x-hypha-order": ["secret_key", "publishable_key", "webhook_secret", "capture_mode"],
            "properties": {
                "secret_key": {
                    "type": "string",
                    "title": "Secret key",
                    "description": "From Stripe's dashboard (Developers → API keys): sk_test_… or sk_live_…, or a restricted rk_ key.",
                    "x-hypha-secret": true
                },
                "publishable_key": {
                    "type": "string",
                    "title": "Publishable key",
                    "description": "pk_test_… or pk_live_…, from the same page: the storefront's payment form uses it."
                },
                "webhook_secret": {
                    "type": "string",
                    "title": "Webhook signing secret",
                    "description": "whsec_…: add an endpoint in Stripe's dashboard pointing at this API's /spores/stripe/webhook, with the payment_intent.* and charge.refunded events.",
                    "x-hypha-secret": true
                },
                "capture_mode": {
                    "type": "string",
                    "title": "Capture",
                    "description": "automatic charges at once; manual authorizes, and staff capture from the order.",
                    "enum": ["automatic", "manual"]
                }
            }
        }))
    }

    fn check_settings(&self, settings: &Map<String, Value>) -> Result<(), Vec<SettingsProblem>> {
        let text = |key: &str| settings.get(key).and_then(Value::as_str);
        let mut problems = Vec::new();
        if let Some(key) = text("secret_key")
            && !(key.starts_with("sk_") || key.starts_with("rk_"))
        {
            problems.push(SettingsProblem::new(
                "secret_key",
                "starts with sk_ (or rk_ for a restricted key)",
            ));
        }
        if let Some(key) = text("publishable_key")
            && !key.starts_with("pk_")
        {
            problems.push(SettingsProblem::new("publishable_key", "starts with pk_"));
        }
        if let Some(secret) = text("webhook_secret")
            && !secret.starts_with("whsec_")
        {
            problems.push(SettingsProblem::new("webhook_secret", "starts with whsec_"));
        }
        // Test keys with live ones would take money with a form that can't.
        let live = |key: &str| key.contains("_live_");
        if let (Some(secret), Some(public)) = (text("secret_key"), text("publishable_key"))
            && live(secret) != live(public)
        {
            problems.push(SettingsProblem::new(
                "publishable_key",
                "is a test key with a live secret key, or the other way round",
            ));
        }
        match problems.is_empty() {
            true => Ok(()),
            false => Err(problems),
        }
    }

    type PaymentProviders = (StripeProvider,);
    type Handlers = (CancelIntents,);

    fn payment_providers(&self) -> (StripeProvider,) {
        (StripeProvider {
            env: self.env.clone(),
            api_base: self.api_base.clone(),
        },)
    }

    fn handlers(&self) -> (CancelIntents,) {
        (CancelIntents {
            env: self.env.clone(),
            api_base: self.api_base.clone(),
        },)
    }
}

hypha_spore::http::spore_routes! { crate::stripe_routes for Stripe:
    [public, POST, "/spores/stripe/webhook", webhook::receive],
    [public, GET, "/spores/stripe/elements/payment.js", element::payment_js],
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(value: Value) -> Map<String, Value> {
        match value {
            Value::Object(map) => map,
            _ => Map::new(),
        }
    }

    fn spore() -> Stripe {
        Stripe::new(hypha_spore::testing::env("stripe")).unwrap()
    }

    #[test]
    fn keys_of_the_right_kinds_pass() {
        let ok = settings(json!({
            "secret_key": "sk_test_1",
            "publishable_key": "pk_test_1",
            "webhook_secret": "whsec_1",
            "capture_mode": "manual"
        }));
        assert_eq!(spore().check_settings(&ok), Ok(()));
    }

    #[test]
    fn each_wrong_key_is_named() {
        let bad = settings(json!({
            "secret_key": "pk_test_1",
            "publishable_key": "sk_test_1",
            "webhook_secret": "secret"
        }));
        let fields: Vec<String> = spore()
            .check_settings(&bad)
            .unwrap_err()
            .into_iter()
            .map(|p| p.field)
            .collect();
        assert_eq!(fields, ["secret_key", "publishable_key", "webhook_secret"]);
    }

    #[test]
    fn test_and_live_keys_dont_mix() {
        let mixed = settings(json!({
            "secret_key": "sk_live_1",
            "publishable_key": "pk_test_1"
        }));
        assert!(spore().check_settings(&mixed).is_err());
    }

    #[test]
    fn settings_read_from_a_map_default_to_automatic_capture() {
        let read = Settings::from_map(&settings(json!({ "secret_key": "sk_test_1" })));
        assert_eq!(read.capture_mode, CaptureMode::Automatic);
        assert!(!read.is_live());
    }
}
