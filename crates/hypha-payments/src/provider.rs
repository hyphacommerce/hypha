//! Payment providers (design note 0027, §3; 0029, §3.1): who collects a payment.
//! `manual` is built in (cash on delivery, bank transfer: staff record what happened);
//! an app collects through the extension points `payments.start`, `.capture`, `.void`
//! and `.refund` under its own key (`app_stripe`); a spore implements `PaymentProvider`
//! in-process under its key (`spore_stripe`, design note 0030, SP1). The host lists the
//! providers it was built with (manual and the spores') in its `payments.rs`, a tuple
//! like its event handlers (static dispatch, so futures stay provably `Send` on concrete
//! types), and answers for apps itself.

use std::fmt;

use hypha_core::{
    AppKey, AppKind, ChannelId, CountryCode, Email, MarketId, Money, OrderId, PaymentId,
    PaymentMethodId, RepoError, Union,
};

use crate::PaymentError;

/// Which provider: `manual`, an app's key `app_<name>` or a spore's `spore_<name>` (the
/// name is lowercase ASCII letters, digits and `_`, starting with a letter; design notes
/// 0029, §10 item 5, and 0030, SP3).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProviderKey(String);

impl ProviderKey {
    pub const MAX_LEN: usize = AppKey::MAX_LEN;
    /// The built-in provider's key.
    pub const MANUAL: &'static str = "manual";

    pub fn new(key: &str) -> Result<Self, PaymentError> {
        if key == Self::MANUAL {
            return Ok(Self::manual());
        }
        match AppKey::new(key) {
            Ok(app) => Ok(Self::of_app(&app)),
            Err(_) => Err(PaymentError::InvalidProviderKey(key.to_string())),
        }
    }

    pub fn manual() -> Self {
        Self(Self::MANUAL.to_string())
    }

    /// The app `app` as a provider: its key is the app's.
    pub fn of_app(app: &AppKey) -> Self {
        Self(app.as_str().to_string())
    }

    pub fn is_manual(&self) -> bool {
        self.0 == Self::MANUAL
    }

    /// The external app that collects for this provider, through the payment points;
    /// `None` for one the host was built with (`manual`, a spore's).
    pub fn app(&self) -> Option<AppKey> {
        AppKey::new(&self.0)
            .ok()
            .filter(|key| key.kind() == AppKind::App)
    }

    /// Whether the app or spore `owner` collects for this provider: an app's provider is
    /// its key; a spore's is its key or its key and `_…` (`spore_stripe_link` is
    /// `spore_stripe`'s), which the hosts check at startup along with no spore name
    /// extending another's (design note 0030, §4). Never `manual`.
    pub fn belongs_to(&self, owner: &AppKey) -> bool {
        let key = owner.as_str();
        match owner.kind() {
            AppKind::App => self.0 == key,
            AppKind::Spore => {
                self.0 == key
                    || self
                        .0
                        .strip_prefix(key)
                        .is_some_and(|rest| rest.starts_with('_'))
            }
        }
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

/// What a provider is asked to collect: the payment checkout just committed (or a retry
/// of it), as plain values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaymentRequest {
    pub payment: PaymentId,
    pub order: OrderId,
    /// The order's human number, e.g. what a bank transfer quotes as its reference.
    pub order_number: i64,
    pub method: PaymentMethodId,
    pub amount: Money,
    /// Where the order was placed, to whom, and where it ships.
    pub channel: ChannelId,
    pub market: MarketId,
    pub country: CountryCode,
    /// The buyer's email. Personal data (design note 0029, AP10): a provider passes it
    /// on only where the rules on personal data allow.
    pub customer_email: Email,
    /// Where the storefront wants the buyer back after a redirect, when it said.
    pub return_url: Option<String>,
    /// The method's instructions as the payment copied them.
    pub instructions: Option<String>,
    /// The method's provider settings.
    pub settings: serde_json::Value,
    /// The provider's store-wide settings, filled by the host for a spore's provider
    /// (design note 0030, §8: what the merchant saved for the spore, secret fields
    /// opened, such as Stripe's secret key); empty for `manual` and for apps, which keep
    /// their own. In memory only, for the provider: never stored or logged.
    pub store_settings: serde_json::Map<String, serde_json::Value>,
}

/// What a provider answered to a start: what the shopper does next, and its own ID for
/// the payment (kept on it as its `reference`), when it has one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Started {
    pub next_action: NextAction,
    pub reference: Option<String>,
}

/// What the shopper must do next (design note 0027, §3; 0029, §7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NextAction {
    /// Nothing online: show the instructions (cash on delivery, bank transfer). Empty
    /// when the method has none.
    Instructions(String),
    /// Mount the app's storefront element (a custom element) with these properties (a
    /// card form's client secret), and wait for its `hypha:payment-confirmed` event.
    ClientWidget {
        element: String,
        props: serde_json::Map<String, serde_json::Value>,
    },
    /// Send the shopper to the provider's page.
    Redirect { url: String },
}

/// What staff ask a provider to do with a payment (design note 0029, §3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PaymentAction {
    /// Collect an authorized payment.
    Capture,
    /// Release a payment before money moved.
    Void,
    /// Give back some or all of a collected one.
    Refund,
}

impl PaymentAction {
    pub fn as_str(self) -> &'static str {
        match self {
            PaymentAction::Capture => "capture",
            PaymentAction::Void => "void",
            PaymentAction::Refund => "refund",
        }
    }
}

impl fmt::Display for PaymentAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// An action for a provider to take on a payment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionRequest {
    pub action: PaymentAction,
    pub payment: PaymentId,
    pub order: OrderId,
    /// For a capture or refund, how much (at most what's left); `None` for the whole.
    pub amount: Option<Money>,
    /// The provider's ID for the payment, as it gave it on start.
    pub reference: Option<String>,
    /// The same for every retry of this action, so a provider does it once.
    pub idempotency_key: String,
    /// As `PaymentRequest::store_settings`: a spore provider's settings, filled by the
    /// host; empty otherwise.
    pub store_settings: serde_json::Map<String, serde_json::Value>,
}

/// Whether the provider took the action. Accepted isn't done: the payment moves when
/// the provider reports what happened (`hypha_order_actions::record_payment_event`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionAnswer {
    Accepted {
        reference: Option<String>,
    },
    /// The provider said no, in words staff are shown.
    Refused {
        reason: String,
    },
}

/// One provider. No `Send` bound, like every port (Workers).
#[allow(async_fn_in_trait)]
pub trait PaymentProvider {
    /// "manual".
    fn key(&self) -> ProviderKey;
    /// Checks a method's settings when staff save it.
    fn check_settings(&self, settings: &serde_json::Value) -> Result<(), PaymentError>;
    /// Starts collecting `payment` (committed with its order): what the shopper must do
    /// next.
    async fn start(&self, payment: &PaymentRequest) -> Result<Started, PaymentError>;
    /// Captures, voids or refunds a payment. A provider that can't (the default):
    /// `payment_action_unsupported`.
    async fn act(&self, request: &ActionRequest) -> Result<ActionAnswer, PaymentError> {
        Err(PaymentError::ActionUnsupported {
            provider: self.key(),
            action: request.action,
        })
    }
}

/// The built-in provider (design note 0027, PA5): nothing to call. Its next action is
/// the method's instructions; staff record the money when it arrives
/// (`POST /admin/orders/{id}/paid`), and cancelling the order voids it. It takes no
/// settings and no actions.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ManualProvider;

impl PaymentProvider for ManualProvider {
    fn key(&self) -> ProviderKey {
        ProviderKey::manual()
    }

    fn check_settings(&self, settings: &serde_json::Value) -> Result<(), PaymentError> {
        no_settings(&self.key(), settings)
    }

    async fn start(&self, payment: &PaymentRequest) -> Result<Started, PaymentError> {
        Ok(Started {
            next_action: NextAction::Instructions(payment.instructions.clone().unwrap_or_default()),
            reference: None,
        })
    }
}

/// The check of a provider that takes no settings (`manual`; an app, which keeps its
/// own, design note 0029, §4.2): `{}` or `null`.
pub fn no_settings(
    provider: &ProviderKey,
    settings: &serde_json::Value,
) -> Result<(), PaymentError> {
    match settings {
        serde_json::Value::Null => Ok(()),
        serde_json::Value::Object(map) if map.is_empty() => Ok(()),
        _ => Err(PaymentError::InvalidSettings {
            provider: provider.clone(),
            reason: format!("{provider} takes no settings: send {{}}"),
        }),
    }
}

/// The providers a host offers: the ones it was built with (a tuple of
/// `PaymentProvider`s, found by key) and, in a host, the store's payment apps.
#[allow(async_fn_in_trait)]
pub trait PaymentProviders {
    /// The keys of the providers this build has, in order (apps aren't listed: they're
    /// found when asked). Each must be unique; the hosts check at startup.
    fn keys(&self) -> Vec<ProviderKey>;
    /// Whether this build has the provider `key`: what a union routes by, without
    /// collecting every key.
    fn has_key(&self, key: &ProviderKey) -> bool {
        self.keys().contains(key)
    }
    /// Whether `key` can collect payments now: a provider this build has, or an
    /// installed, enabled app that implements `payments.start`.
    async fn available(&self, key: &ProviderKey) -> Result<bool, RepoError>;
    /// The provider `key`'s check of a method's settings; `UnknownProvider` if there's
    /// no such provider.
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
    ) -> Result<Started, PaymentError>;
    /// The provider `key`'s `act`; `UnknownProvider` if there's none.
    async fn act(
        &self,
        key: &ProviderKey,
        request: &ActionRequest,
    ) -> Result<ActionAnswer, PaymentError>;
}

impl<T: PaymentProviders + ?Sized> PaymentProviders for &T {
    fn keys(&self) -> Vec<ProviderKey> {
        (**self).keys()
    }

    fn has_key(&self, key: &ProviderKey) -> bool {
        (**self).has_key(key)
    }

    async fn available(&self, key: &ProviderKey) -> Result<bool, RepoError> {
        (**self).available(key).await
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
    ) -> Result<Started, PaymentError> {
        (**self).start(key, payment).await
    }

    async fn act(
        &self,
        key: &ProviderKey,
        request: &ActionRequest,
    ) -> Result<ActionAnswer, PaymentError> {
        (**self).act(key, request).await
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

            fn has_key(&self, key: &ProviderKey) -> bool {
                let ($($p,)+) = self;
                false $(|| &$p.key() == key)+
            }

            async fn available(&self, key: &ProviderKey) -> Result<bool, RepoError> {
                let ($($p,)+) = self;
                Ok(false $(|| &$p.key() == key)+)
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
            ) -> Result<Started, PaymentError> {
                let ($($p,)+) = self;
                $(
                    if &$p.key() == key {
                        return $p.start(payment).await;
                    }
                )+
                Err(PaymentError::UnknownProvider(key.clone()))
            }

            async fn act(
                &self,
                key: &ProviderKey,
                request: &ActionRequest,
            ) -> Result<ActionAnswer, PaymentError> {
                let ($($p,)+) = self;
                $(
                    if &$p.key() == key {
                        return $p.act(request).await;
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

/// No providers: a spore that contributes none (design note 0030, §4).
impl PaymentProviders for () {
    fn keys(&self) -> Vec<ProviderKey> {
        Vec::new()
    }

    fn has_key(&self, _: &ProviderKey) -> bool {
        false
    }

    async fn available(&self, _: &ProviderKey) -> Result<bool, RepoError> {
        Ok(false)
    }

    fn check_settings(&self, key: &ProviderKey, _: &serde_json::Value) -> Result<(), PaymentError> {
        Err(PaymentError::UnknownProvider(key.clone()))
    }

    async fn start(&self, key: &ProviderKey, _: &PaymentRequest) -> Result<Started, PaymentError> {
        Err(PaymentError::UnknownProvider(key.clone()))
    }

    async fn act(
        &self,
        key: &ProviderKey,
        _: &ActionRequest,
    ) -> Result<ActionAnswer, PaymentError> {
        Err(PaymentError::UnknownProvider(key.clone()))
    }
}

/// `PaymentProviders` for a union of 1 to 8 sets (`hypha_core::Union`): a host's own
/// providers and each spore's, read as one set, the first set with the key answering
/// (design note 0030, §4).
macro_rules! union_providers {
    ($($s:ident),+) => {
        #[allow(non_snake_case)]
        impl<$($s: PaymentProviders),+> PaymentProviders for Union<($($s,)+)> {
            fn keys(&self) -> Vec<ProviderKey> {
                let Union(($($s,)+)) = self;
                let mut keys = Vec::new();
                $(keys.extend($s.keys());)+
                keys
            }

            fn has_key(&self, key: &ProviderKey) -> bool {
                let Union(($($s,)+)) = self;
                false $(|| $s.has_key(key))+
            }

            async fn available(&self, key: &ProviderKey) -> Result<bool, RepoError> {
                let Union(($($s,)+)) = self;
                $(
                    if $s.has_key(key) {
                        return $s.available(key).await;
                    }
                )+
                Ok(false)
            }

            fn check_settings(
                &self,
                key: &ProviderKey,
                settings: &serde_json::Value,
            ) -> Result<(), PaymentError> {
                let Union(($($s,)+)) = self;
                $(
                    if $s.has_key(key) {
                        return $s.check_settings(key, settings);
                    }
                )+
                Err(PaymentError::UnknownProvider(key.clone()))
            }

            async fn start(
                &self,
                key: &ProviderKey,
                payment: &PaymentRequest,
            ) -> Result<Started, PaymentError> {
                let Union(($($s,)+)) = self;
                $(
                    if $s.has_key(key) {
                        return $s.start(key, payment).await;
                    }
                )+
                Err(PaymentError::UnknownProvider(key.clone()))
            }

            async fn act(
                &self,
                key: &ProviderKey,
                request: &ActionRequest,
            ) -> Result<ActionAnswer, PaymentError> {
                let Union(($($s,)+)) = self;
                $(
                    if $s.has_key(key) {
                        return $s.act(key, request).await;
                    }
                )+
                Err(PaymentError::UnknownProvider(key.clone()))
            }
        }
    };
}

union_providers!(A);
union_providers!(A, B);
union_providers!(A, B, C);
union_providers!(A, B, C, D);
union_providers!(A, B, C, D, E);
union_providers!(A, B, C, D, E, F);
union_providers!(A, B, C, D, E, F, G);
union_providers!(A, B, C, D, E, F, G, H);

#[cfg(test)]
mod tests {
    use hypha_core::Currency;
    use uuid::Uuid;

    use super::*;

    #[test]
    fn keys_are_manual_an_app_or_a_spore() {
        assert_eq!(ProviderKey::new("manual").map(|k| k.is_manual()), Ok(true));
        let stripe = ProviderKey::new("app_stripe").unwrap();
        assert_eq!(stripe.to_string(), "app_stripe");
        assert_eq!(stripe.app(), Some(AppKey::new("app_stripe").unwrap()));
        assert_eq!(ProviderKey::manual().app(), None);
        // A spore's provider is built in: no app to call.
        let spore = ProviderKey::new("spore_stripe").unwrap();
        assert_eq!(spore.to_string(), "spore_stripe");
        assert_eq!(spore.app(), None);
        for bad in [
            "",
            "stripe",
            "spores_stripe",
            "app_",
            "app_9x",
            "app_Stripe",
            "Manual",
        ] {
            assert_eq!(
                ProviderKey::new(bad),
                Err(PaymentError::InvalidProviderKey(bad.to_string())),
                "{bad:?}"
            );
        }
        let long = format!("app_{}", "a".repeat(ProviderKey::MAX_LEN - 4));
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
            channel: ChannelId::from_uuid(Uuid::from_u128(4)),
            market: MarketId::from_uuid(Uuid::from_u128(5)),
            country: CountryCode::new("HR").unwrap(),
            customer_email: Email::new("buyer@example.com").unwrap(),
            return_url: None,
            instructions: instructions.map(str::to_string),
            settings: serde_json::json!({}),
            store_settings: serde_json::Map::new(),
        }
    }

    fn instructions(text: &str) -> Started {
        Started {
            next_action: NextAction::Instructions(text.to_string()),
            reference: None,
        }
    }

    #[test]
    fn manual_answers_with_the_instructions() {
        assert_eq!(
            pollster::block_on(ManualProvider.start(&request(Some("IBAN HR12")))),
            Ok(instructions("IBAN HR12"))
        );
        assert_eq!(
            pollster::block_on(ManualProvider.start(&request(None))),
            Ok(instructions(""))
        );
    }

    #[test]
    fn manual_takes_no_actions() {
        let action = ActionRequest {
            action: PaymentAction::Capture,
            payment: PaymentId::from_uuid(Uuid::from_u128(1)),
            order: OrderId::from_uuid(Uuid::from_u128(2)),
            amount: None,
            reference: None,
            idempotency_key: "k".to_string(),
            store_settings: serde_json::Map::new(),
        };
        assert_eq!(
            pollster::block_on(ManualProvider.act(&action)),
            Err(PaymentError::ActionUnsupported {
                provider: ProviderKey::manual(),
                action: PaymentAction::Capture
            })
        );
    }

    #[test]
    fn a_tuple_finds_its_provider_by_key() {
        let providers = (ManualProvider,);
        assert_eq!(
            pollster::block_on(providers.available(&ProviderKey::manual())),
            Ok(true)
        );
        let stripe = ProviderKey::new("app_stripe").unwrap();
        assert_eq!(pollster::block_on(providers.available(&stripe)), Ok(false));
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
            Ok(instructions("x"))
        );
    }

    /// A spore's provider, answering with its own name.
    struct Spore;

    impl PaymentProvider for Spore {
        fn key(&self) -> ProviderKey {
            ProviderKey::new("spore_test").unwrap()
        }

        fn check_settings(&self, _: &serde_json::Value) -> Result<(), PaymentError> {
            Ok(())
        }

        async fn start(&self, _: &PaymentRequest) -> Result<Started, PaymentError> {
            Ok(instructions("spore"))
        }
    }

    #[test]
    fn a_provider_belongs_to_its_app_or_its_spore() {
        let key = |k: &str| ProviderKey::new(k).unwrap();
        let owner = |k: &str| AppKey::new(k).unwrap();
        assert!(key("app_stripe").belongs_to(&owner("app_stripe")));
        assert!(!key("app_stripe_link").belongs_to(&owner("app_stripe")));
        assert!(key("spore_stripe").belongs_to(&owner("spore_stripe")));
        assert!(key("spore_stripe_link").belongs_to(&owner("spore_stripe")));
        assert!(!key("spore_stripes").belongs_to(&owner("spore_stripe")));
        assert!(!key("spore_stripe").belongs_to(&owner("app_stripe")));
        assert!(!ProviderKey::manual().belongs_to(&owner("app_manual")));
    }

    #[test]
    fn nothing_has_no_providers() {
        assert_eq!(().keys(), []);
        assert_eq!(
            pollster::block_on(().available(&ProviderKey::manual())),
            Ok(false)
        );
        assert_eq!(
            pollster::block_on(().start(&ProviderKey::manual(), &request(None))),
            Err(PaymentError::UnknownProvider(ProviderKey::manual()))
        );
    }

    #[test]
    fn a_union_reads_as_one_set() {
        let spore = Spore.key();
        let providers = Union(((ManualProvider,), (), Union(((Spore,),))));
        assert_eq!(providers.keys(), [ProviderKey::manual(), spore.clone()]);
        assert!(providers.has_key(&spore));
        assert!(providers.has_key(&ProviderKey::manual()));
        assert!(!providers.has_key(&ProviderKey::new("app_stripe").unwrap()));
        assert_eq!(pollster::block_on(providers.available(&spore)), Ok(true));
        assert_eq!(
            pollster::block_on(providers.start(&spore, &request(Some("x")))),
            Ok(instructions("spore"))
        );
        assert_eq!(
            pollster::block_on(providers.start(&ProviderKey::manual(), &request(Some("x")))),
            Ok(instructions("x"))
        );
        assert_eq!(
            providers.check_settings(&spore, &serde_json::json!({"a": 1})),
            Ok(())
        );
        let stripe = ProviderKey::new("app_stripe").unwrap();
        assert_eq!(pollster::block_on(providers.available(&stripe)), Ok(false));
        assert_eq!(
            pollster::block_on(providers.start(&stripe, &request(None))),
            Err(PaymentError::UnknownProvider(stripe))
        );
    }
}
