//! Payment providers (design note 0027, §3): who collects a payment. `manual` is
//! built in (cash on delivery, bank transfer: staff record what happened); a spore adds
//! its own (`spore_stripe`). The host lists the providers it was built with in its
//! `payments.rs`, a tuple like its event handlers: static dispatch, so futures stay
//! provably `Send` on concrete types.

use std::fmt;

use hypha_core::{Money, OrderId, PaymentId, PaymentMethodId};

use crate::PaymentError;

/// Which provider: `manual`, or a compiled-in spore's `spore_<name>` (the name is
/// lowercase ASCII letters, digits and `_`, starting with a letter).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProviderKey(String);

impl ProviderKey {
    pub const MAX_LEN: usize = 64;
    /// The built-in provider's key.
    pub const MANUAL: &'static str = "manual";

    pub fn new(key: &str) -> Result<Self, PaymentError> {
        if key == Self::MANUAL {
            return Ok(Self::manual());
        }
        let invalid = || PaymentError::InvalidProviderKey(key.to_string());
        let name = key.strip_prefix("spore_").ok_or_else(invalid)?;
        let mut chars = name.chars();
        let valid = key.len() <= Self::MAX_LEN
            && chars.next().is_some_and(|c| c.is_ascii_lowercase())
            && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        if !valid {
            return Err(invalid());
        }
        Ok(Self(key.to_string()))
    }

    pub fn manual() -> Self {
        Self(Self::MANUAL.to_string())
    }

    pub fn is_manual(&self) -> bool {
        self.0 == Self::MANUAL
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ProviderKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What a provider is asked to collect: the payment checkout just committed, as plain
/// values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaymentRequest {
    pub payment: PaymentId,
    pub order: OrderId,
    /// The order's human number, e.g. what a bank transfer quotes as its reference.
    pub order_number: i64,
    pub method: PaymentMethodId,
    pub amount: Money,
    /// The method's instructions as the payment copied them.
    pub instructions: Option<String>,
    /// The method's provider settings.
    pub settings: serde_json::Value,
}

/// What the shopper must do next (design note 0027, §3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NextAction {
    /// Nothing online: show the instructions (cash on delivery, bank transfer). Empty
    /// when the method has none.
    Instructions(String),
    /// A provider's page or widget: card fields.
    ClientSecret {
        secret: String,
    },
    Redirect {
        url: String,
    },
}

/// One provider. No `Send` bound, like every port (Workers).
#[allow(async_fn_in_trait)]
pub trait PaymentProvider {
    /// "manual", "spore_stripe".
    fn key(&self) -> ProviderKey;
    /// Checks a method's settings when staff save it.
    fn check_settings(&self, settings: &serde_json::Value) -> Result<(), PaymentError>;
    /// Starts collecting `payment` (committed with its order): what the shopper must do
    /// next.
    async fn start(&self, payment: &PaymentRequest) -> Result<NextAction, PaymentError>;
}

/// The built-in provider (design note 0027, PA5): nothing to call. Its next action is
/// the method's instructions; staff record the money when it arrives
/// (`POST /admin/orders/{id}/paid`). It takes no settings.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ManualProvider;

impl PaymentProvider for ManualProvider {
    fn key(&self) -> ProviderKey {
        ProviderKey::manual()
    }

    fn check_settings(&self, settings: &serde_json::Value) -> Result<(), PaymentError> {
        match settings {
            serde_json::Value::Null => Ok(()),
            serde_json::Value::Object(map) if map.is_empty() => Ok(()),
            _ => Err(PaymentError::InvalidSettings {
                provider: self.key(),
                reason: "the manual provider takes no settings: send {}".to_string(),
            }),
        }
    }

    async fn start(&self, payment: &PaymentRequest) -> Result<NextAction, PaymentError> {
        Ok(NextAction::Instructions(
            payment.instructions.clone().unwrap_or_default(),
        ))
    }
}

/// The providers a host was built with: a tuple of `PaymentProvider`s, found by key.
#[allow(async_fn_in_trait)]
pub trait PaymentProviders {
    /// Every provider's key, in order.
    fn keys(&self) -> Vec<ProviderKey>;
    /// Whether one has `key`.
    fn has(&self, key: &ProviderKey) -> bool {
        self.keys().contains(key)
    }
    /// The provider `key`'s check; `UnknownProvider` if there's none.
    fn check_settings(
        &self,
        key: &ProviderKey,
        settings: &serde_json::Value,
    ) -> Result<(), PaymentError>;
    /// The provider `key`'s `start`; `UnknownProvider` if there's none.
    async fn start(
        &self,
        key: &ProviderKey,
        payment: &PaymentRequest,
    ) -> Result<NextAction, PaymentError>;
}

impl<T: PaymentProviders + ?Sized> PaymentProviders for &T {
    fn keys(&self) -> Vec<ProviderKey> {
        (**self).keys()
    }

    fn check_settings(
        &self,
        key: &ProviderKey,
        settings: &serde_json::Value,
    ) -> Result<(), PaymentError> {
        (**self).check_settings(key, settings)
    }

    async fn start(
        &self,
        key: &ProviderKey,
        payment: &PaymentRequest,
    ) -> Result<NextAction, PaymentError> {
        (**self).start(key, payment).await
    }
}

/// `PaymentProviders` for tuples of 1 to 8 providers. The type parameters double as the
/// names of the tuple's fields when it's destructured.
macro_rules! tuple_providers {
    ($($p:ident),+) => {
        #[allow(non_snake_case)]
        impl<$($p: PaymentProvider),+> PaymentProviders for ($($p,)+) {
            fn keys(&self) -> Vec<ProviderKey> {
                let ($($p,)+) = self;
                vec![$($p.key()),+]
            }

            fn check_settings(
                &self,
                key: &ProviderKey,
                settings: &serde_json::Value,
            ) -> Result<(), PaymentError> {
                let ($($p,)+) = self;
                $(
                    if &$p.key() == key {
                        return $p.check_settings(settings);
                    }
                )+
                Err(PaymentError::UnknownProvider(key.clone()))
            }

            async fn start(
                &self,
                key: &ProviderKey,
                payment: &PaymentRequest,
            ) -> Result<NextAction, PaymentError> {
                let ($($p,)+) = self;
                $(
                    if &$p.key() == key {
                        return $p.start(payment).await;
                    }
                )+
                Err(PaymentError::UnknownProvider(key.clone()))
            }
        }
    };
}

tuple_providers!(A);
tuple_providers!(A, B);
tuple_providers!(A, B, C);
tuple_providers!(A, B, C, D);
tuple_providers!(A, B, C, D, E);
tuple_providers!(A, B, C, D, E, F);
tuple_providers!(A, B, C, D, E, F, G);
tuple_providers!(A, B, C, D, E, F, G, H);

#[cfg(test)]
mod tests {
    use hypha_core::Currency;
    use uuid::Uuid;

    use super::*;

    #[test]
    fn keys_are_manual_or_a_spore() {
        assert_eq!(ProviderKey::new("manual").map(|k| k.is_manual()), Ok(true));
        assert_eq!(
            ProviderKey::new("spore_stripe").map(|k| k.to_string()),
            Ok("spore_stripe".to_string())
        );
        for bad in [
            "",
            "stripe",
            "app_stripe",
            "spore_",
            "spore_9x",
            "spore_Stripe",
            "Manual",
        ] {
            assert_eq!(
                ProviderKey::new(bad),
                Err(PaymentError::InvalidProviderKey(bad.to_string())),
                "{bad:?}"
            );
        }
        let long = format!("spore_{}", "a".repeat(ProviderKey::MAX_LEN - 6));
        assert!(ProviderKey::new(&long).is_ok());
        assert!(ProviderKey::new(&format!("{long}a")).is_err());
    }

    #[test]
    fn manual_takes_no_settings() {
        assert_eq!(
            ManualProvider.check_settings(&serde_json::json!({})),
            Ok(())
        );
        assert_eq!(
            ManualProvider.check_settings(&serde_json::Value::Null),
            Ok(())
        );
        assert!(matches!(
            ManualProvider.check_settings(&serde_json::json!({"iban": "x"})),
            Err(PaymentError::InvalidSettings { .. })
        ));
    }

    fn request(instructions: Option<&str>) -> PaymentRequest {
        PaymentRequest {
            payment: PaymentId::from_uuid(Uuid::from_u128(1)),
            order: OrderId::from_uuid(Uuid::from_u128(2)),
            order_number: 1001,
            method: PaymentMethodId::from_uuid(Uuid::from_u128(3)),
            amount: Money::new(1000, Currency::EUR).unwrap(),
            instructions: instructions.map(str::to_string),
            settings: serde_json::json!({}),
        }
    }

    #[test]
    fn manual_answers_with_the_instructions() {
        assert_eq!(
            pollster::block_on(ManualProvider.start(&request(Some("IBAN HR12")))),
            Ok(NextAction::Instructions("IBAN HR12".to_string()))
        );
        assert_eq!(
            pollster::block_on(ManualProvider.start(&request(None))),
            Ok(NextAction::Instructions(String::new()))
        );
    }

    #[test]
    fn a_tuple_finds_its_provider_by_key() {
        let providers = (ManualProvider,);
        assert_eq!(providers.keys(), [ProviderKey::manual()]);
        assert!(providers.has(&ProviderKey::manual()));
        let stripe = ProviderKey::new("spore_stripe").unwrap();
        assert!(!providers.has(&stripe));
        assert_eq!(
            providers.check_settings(&stripe, &serde_json::json!({})),
            Err(PaymentError::UnknownProvider(stripe.clone()))
        );
        assert_eq!(
            pollster::block_on(providers.start(&stripe, &request(None))),
            Err(PaymentError::UnknownProvider(stripe))
        );
        assert_eq!(
            pollster::block_on(providers.start(&ProviderKey::manual(), &request(Some("x")))),
            Ok(NextAction::Instructions("x".to_string()))
        );
    }
}
