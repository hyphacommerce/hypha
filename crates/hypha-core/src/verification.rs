//! Verifying an address with a provider, through the store's verification app: whether
//! mail reaches it, what the provider would correct, and what it is (a PO box, a home).
//! The answer is kept with the cart and the order it was asked for, so the same address
//! is never looked up twice and checkout reads what's kept.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{Address, AddressVerdict, AppKey, RepoError, Timestamp};

/// How sure the provider is of one field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum FieldStatus {
    /// As written.
    Confirmed,
    /// Written otherwise, and corrected.
    Corrected,
    /// Not written, and filled in.
    Inferred,
    /// Written, but not found.
    Unconfirmed,
    /// Needed, and not written.
    Missing,
}

/// How far down the provider found the address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum AddressPrecision {
    Country,
    Locality,
    Street,
    /// The building.
    Premise,
    /// The apartment or unit.
    Subpremise,
}

/// What the provider says the address is; `None` where it doesn't know.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct VerificationFlags {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "openapi", schema(required = false))]
    pub po_box: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "openapi", schema(required = false))]
    pub residential: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "openapi", schema(required = false))]
    pub business: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "openapi", schema(required = false))]
    pub vacant: Option<bool>,
}

/// An address as a provider suggests it: where it is, without who lives there (names and
/// phone are never sent, so never suggested).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SuggestedAddress {
    #[serde(default)]
    pub company: Option<String>,
    pub line1: String,
    #[serde(default)]
    pub line2: Option<String>,
    pub city: String,
    #[serde(default)]
    pub province: Option<String>,
    #[serde(default)]
    pub postal_code: Option<String>,
    pub country: String,
}

/// Where an address is verified: setting a cart's, or staff editing an order's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum VerifyContext {
    Cart,
    Order,
}

impl VerifyContext {
    pub fn as_str(self) -> &'static str {
        match self {
            VerifyContext::Cart => "cart",
            VerifyContext::Order => "order",
        }
    }
}

/// A verification as kept: what was asked (by the fields' digest), the answer, who
/// answered and when. Stored as JSON with carts and orders, so its shape is storage's: a
/// field is added, never renamed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddressVerification {
    /// The digest of the address fields sent: setting the same address again asks nobody.
    pub digest: String,
    pub verdict: AddressVerdict,
    #[serde(default)]
    pub precision: Option<AddressPrecision>,
    /// Per field (`postal_code`), how sure the provider is.
    #[serde(default)]
    pub fields: BTreeMap<String, FieldStatus>,
    /// The corrected address, only when the provider is confident; same country.
    #[serde(default)]
    pub suggested: Option<SuggestedAddress>,
    /// The suggestion's digest: setting it is deliverable without a second lookup.
    #[serde(default)]
    pub suggested_digest: Option<String>,
    #[serde(default)]
    pub flags: VerificationFlags,
    /// The app that answered (or failed to).
    #[serde(default, with = "app_key_opt")]
    pub app: Option<AppKey>,
    /// The provider's own ID for the lookup.
    #[serde(default)]
    pub reference: Option<String>,
    /// Why the verdict is `unknown` when the app couldn't be asked or its answer was
    /// refused; the extension call log has the details.
    #[serde(default)]
    pub failure: Option<String>,
    #[serde(with = "crate::time::serde_micros")]
    pub checked_at: Timestamp,
}

impl AddressVerification {
    /// The app couldn't be asked, or answered what the point refuses: the address is
    /// unknown, which is never the shopper's problem.
    pub fn failed(digest: String, app: AppKey, why: String, at: Timestamp) -> Self {
        Self {
            digest,
            verdict: AddressVerdict::Unknown,
            precision: None,
            fields: BTreeMap::new(),
            suggested: None,
            suggested_digest: None,
            flags: VerificationFlags::default(),
            app: Some(app),
            reference: None,
            failure: Some(why),
            checked_at: at,
        }
    }

    /// What's kept says about the address with `digest` without asking again: the same
    /// address, or the suggestion accepted (deliverable, as the provider suggested it). A
    /// failure is asked again.
    pub fn reusable_for(&self, digest: &str) -> Option<Self> {
        if self.failure.is_some() {
            return None;
        }
        if self.digest == digest {
            return Some(self.clone());
        }
        if self.suggested_digest.as_deref() == Some(digest) {
            return Some(Self {
                digest: digest.to_string(),
                verdict: AddressVerdict::Deliverable,
                fields: BTreeMap::new(),
                suggested: None,
                suggested_digest: None,
                ..self.clone()
            });
        }
        None
    }

    /// Whether staff should look at the address before shipping: the provider couldn't
    /// place it, or not as one place.
    pub fn needs_review(&self) -> bool {
        matches!(
            self.verdict,
            AddressVerdict::Ambiguous | AddressVerdict::Undeliverable
        )
    }
}

/// The store's address verification, as the host provides it: the store's verification
/// app, asked through its extension point. No `Send` bound, like every port.
#[allow(async_fn_in_trait)]
pub trait AddressVerifier {
    /// The digest of the fields a verification sends (not names or phone), which a kept
    /// verification is found by.
    fn digest(&self, address: &Address) -> String;

    /// Asks the store's verification app about `address`; `None` when the store has none.
    /// The app failing is an `unknown` verdict with why, never an error: only reading the
    /// store's settings can fail.
    async fn verify(
        &self,
        address: &Address,
        digest: &str,
        context: VerifyContext,
    ) -> Result<Option<AddressVerification>, RepoError>;
}

impl<T: AddressVerifier + ?Sized> AddressVerifier for &T {
    fn digest(&self, address: &Address) -> String {
        (**self).digest(address)
    }

    async fn verify(
        &self,
        address: &Address,
        digest: &str,
        context: VerifyContext,
    ) -> Result<Option<AddressVerification>, RepoError> {
        (**self).verify(address, digest, context).await
    }
}

/// No verification: a store without a verification app, and tests that don't ask.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoVerification;

impl AddressVerifier for NoVerification {
    fn digest(&self, _address: &Address) -> String {
        String::new()
    }

    async fn verify(
        &self,
        _address: &Address,
        _digest: &str,
        _context: VerifyContext,
    ) -> Result<Option<AddressVerification>, RepoError> {
        Ok(None)
    }
}

/// The verification of a cart's `address`: what `previous` says when it covers the same
/// address (or its accepted suggestion), else the app asked now.
pub async fn verify_address(
    verifier: &impl AddressVerifier,
    address: &Address,
    previous: Option<&AddressVerification>,
    context: VerifyContext,
) -> Result<Option<AddressVerification>, RepoError> {
    let digest = verifier.digest(address);
    if let Some(kept) = previous.and_then(|p| p.reusable_for(&digest)) {
        return Ok(Some(kept));
    }
    verifier.verify(address, &digest, context).await
}

/// An optional `AppKey` as its string.
mod app_key_opt {
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    use crate::AppKey;

    pub(super) fn serialize<S: Serializer>(key: &Option<AppKey>, s: S) -> Result<S::Ok, S::Error> {
        match key {
            Some(key) => s.serialize_some(key.as_str()),
            None => s.serialize_none(),
        }
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<AppKey>, D::Error> {
        Option::<String>::deserialize(d)?
            .map(|raw| AppKey::new(&raw).map_err(D::Error::custom))
            .transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kept() -> AddressVerification {
        AddressVerification {
            digest: "a".to_string(),
            verdict: AddressVerdict::DeliverableWithFixes,
            precision: Some(AddressPrecision::Premise),
            fields: BTreeMap::from([("postal_code".to_string(), FieldStatus::Corrected)]),
            suggested: Some(SuggestedAddress {
                company: None,
                line1: "1 Main St".to_string(),
                line2: None,
                city: "Springfield".to_string(),
                province: Some("IL".to_string()),
                postal_code: Some("62701-1234".to_string()),
                country: "US".to_string(),
            }),
            suggested_digest: Some("b".to_string()),
            flags: VerificationFlags {
                residential: Some(true),
                ..VerificationFlags::default()
            },
            app: Some(AppKey::new("app_verify").unwrap()),
            reference: Some("ref-1".to_string()),
            failure: None,
            checked_at: Timestamp::from_unix_micros(5),
        }
    }

    #[test]
    fn the_same_address_reuses_the_answer() {
        assert_eq!(kept().reusable_for("a"), Some(kept()));
    }

    #[test]
    fn the_accepted_suggestion_is_deliverable_without_a_lookup() {
        let reused = kept().reusable_for("b").unwrap();
        assert_eq!(reused.verdict, AddressVerdict::Deliverable);
        assert_eq!(reused.digest, "b");
        assert_eq!(reused.suggested, None);
        assert_eq!(reused.flags, kept().flags);
    }

    #[test]
    fn another_address_is_asked_again() {
        assert_eq!(kept().reusable_for("c"), None);
    }

    #[test]
    fn a_failure_is_asked_again() {
        let failed = AddressVerification::failed(
            "a".to_string(),
            AppKey::new("app_verify").unwrap(),
            "timeout".to_string(),
            Timestamp::from_unix_micros(1),
        );
        assert_eq!(failed.reusable_for("a"), None);
    }

    #[test]
    fn ambiguous_and_undeliverable_need_review() {
        let review: Vec<AddressVerdict> = AddressVerdict::ALL
            .into_iter()
            .filter(|v| {
                AddressVerification {
                    verdict: *v,
                    ..kept()
                }
                .needs_review()
            })
            .collect();
        assert_eq!(
            review,
            vec![AddressVerdict::Ambiguous, AddressVerdict::Undeliverable]
        );
    }

    #[test]
    fn it_round_trips_through_json() {
        let json = serde_json::to_value(kept()).unwrap();
        assert_eq!(json["checked_at"], 5);
        assert_eq!(json["app"], "app_verify");
        let back: AddressVerification = serde_json::from_value(json).unwrap();
        assert_eq!(back, kept());
    }

    #[test]
    fn contexts_have_stable_names() {
        assert_eq!(VerifyContext::Cart.as_str(), "cart");
        assert_eq!(VerifyContext::Order.as_str(), "order");
    }
}
