//! The host's `AddressVerifier`: the store's verification app (`CheckoutSettings`), asked
//! through `address.verify` v1 with every call in its call log. The point falls back: an
//! app that fails leaves the address `unknown`, with why, and checkout goes on.

use hypha_addresses::Formats;
use hypha_apps::{HasApps, HasExtensionEndpoints};
use hypha_checkout_rules::{CheckoutSettingsRepo, HasCheckoutSettings};
use hypha_core::{
    Address, AddressFormats, AddressParts, AddressVerification, AddressVerifier, AppKey, Clock,
    RepoError, SuggestedAddress, VerifyContext,
};
use hypha_extensions::{
    AddressVerify, AddressVerifyRequest, AddressVerifyResponse, Answered, ExtensionPoint,
    HasExtensionCalls, VerifiedAddress, apply_policy, call,
};
use sha2::{Digest, Sha256};

use crate::{
    app_endpoints::{AppEndpoints, EndpointError, HostAppEndpoints},
    extensions::HostExtensions,
    host::HostClock,
    state::HostEnv,
};

/// One verifier per request: the store's settings and apps, the call log, the shared
/// HTTP client.
pub struct HostVerifier<'a, P> {
    pub ports: &'a P,
    pub endpoints: HostAppEndpoints<'a, P>,
    pub caller: HostExtensions<'a>,
    pub env: &'a HostEnv,
    pub clock: &'a HostClock,
    /// What the calls are for (the cart, the order), recorded with each.
    pub correlation: Option<String>,
}

impl<P> AddressVerifier for HostVerifier<'_, P>
where
    P: HasCheckoutSettings + HasApps + HasExtensionEndpoints + HasExtensionCalls,
{
    fn digest(&self, address: &Address) -> String {
        let p = address.parts();
        digest_of(
            p.company.as_deref(),
            &p.line1,
            p.line2.as_deref(),
            &p.city,
            p.province.as_deref(),
            p.postal_code.as_deref(),
            &p.country,
        )
    }

    async fn app(&self) -> Result<Option<AppKey>, RepoError> {
        let settings = self.ports.checkout_settings().get().await?;
        Ok(settings.and_then(|s| s.verification_app().cloned()))
    }

    async fn ask(
        &self,
        app: &AppKey,
        address: &Address,
        digest: &str,
        context: VerifyContext,
    ) -> Result<AddressVerification, RepoError> {
        let app = app.clone();
        let now = self.clock.now();
        let failed =
            |why: String| AddressVerification::failed(digest.to_string(), app.clone(), why, now);
        let endpoint = match self.endpoints.endpoint(&app, AddressVerify::NAME).await {
            Ok(Some(found)) => found,
            Ok(None) => {
                return Ok(failed(
                    "it doesn't answer address.verify (any more)".to_string(),
                ));
            }
            Err(EndpointError::Repo(e)) => return Err(e),
            Err(e @ EndpointError::Secret(_)) => return Ok(failed(e.to_string())),
        };
        let p = address.parts();
        let request = AddressVerifyRequest {
            address: VerifiedAddress {
                company: p.company.clone(),
                line1: p.line1.clone(),
                line2: p.line2.clone(),
                city: p.city.clone(),
                province: p.province.clone(),
                postal_code: p.postal_code.clone(),
                country: p.country.clone(),
            },
            context,
        };
        // The same address is the same question: an app may answer a retry from its cache.
        let key = format!("verify_{}", &digest[..digest.len().min(32)]);
        let answered = apply_policy::<AddressVerify>(
            call::<AddressVerify>(
                &self.caller,
                self.ports.extension_calls(),
                self.env,
                &endpoint.as_endpoint(),
                &request,
                &key,
                self.correlation.as_deref(),
            )
            .await,
        );
        Ok(match answered {
            Ok(Answered::App(answer)) => kept(digest, app, answer, now),
            Ok(Answered::Fallback(e)) | Err(e) => {
                tracing::warn!(%app, point = AddressVerify::NAME, why = %e,
                    "address verification failed; the address is kept as unknown");
                failed(e.to_string())
            }
        })
    }
}

/// The answer as kept: the suggestion normalised like any address a shopper sets, with
/// its digest, so accepting it asks nobody again.
fn kept(
    digest: &str,
    app: AppKey,
    answer: AddressVerifyResponse,
    now: hypha_core::Timestamp,
) -> AddressVerification {
    let suggested = answer.suggested.map(normalised);
    let suggested_digest = suggested.as_ref().map(|s| {
        digest_of(
            s.company.as_deref(),
            &s.line1,
            s.line2.as_deref(),
            &s.city,
            s.province.as_deref(),
            s.postal_code.as_deref(),
            &s.country,
        )
    });
    AddressVerification {
        digest: digest.to_string(),
        verdict: answer.verdict,
        precision: answer.precision,
        fields: answer
            .fields
            .into_iter()
            .map(|(field, v)| (field, v.status))
            .collect(),
        suggested,
        suggested_digest,
        flags: answer.flags,
        app: Some(app),
        reference: answer.reference,
        failure: None,
        checked_at: now,
    }
}

/// A suggestion as its country's format writes it (the region as its code, the postal
/// code upper-cased), or as given when it doesn't make an address.
fn normalised(s: SuggestedAddress) -> SuggestedAddress {
    let parts = AddressParts {
        first_name: None,
        // Names aren't part of a suggestion; one is needed to build an address.
        last_name: "-".to_string(),
        company: s.company.clone(),
        line1: s.line1.clone(),
        line2: s.line2.clone(),
        city: s.city.clone(),
        province: s.province.clone(),
        postal_code: s.postal_code.clone(),
        country: s.country.to_ascii_uppercase(),
        phone: None,
    };
    let Ok(address) = Address::new(parts) else {
        return s;
    };
    let p = Formats.check(&address).address.parts().clone();
    SuggestedAddress {
        company: p.company,
        line1: p.line1,
        line2: p.line2,
        city: p.city,
        province: p.province,
        postal_code: p.postal_code,
        country: p.country,
    }
}

/// SHA-256 over the fields a verification sends, each ended by a unit separator so no
/// two addresses run together the same way.
fn digest_of(
    company: Option<&str>,
    line1: &str,
    line2: Option<&str>,
    city: &str,
    province: Option<&str>,
    postal_code: Option<&str>,
    country: &str,
) -> String {
    let mut hash = Sha256::new();
    for part in [
        company.unwrap_or_default(),
        line1,
        line2.unwrap_or_default(),
        city,
        province.unwrap_or_default(),
        postal_code.unwrap_or_default(),
        country,
    ] {
        hash.update(part.as_bytes());
        hash.update([0x1f]);
    }
    hash.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkout_rules_names_the_point_this_build_calls() {
        assert_eq!(hypha_checkout_rules::VERIFY_POINT, AddressVerify::NAME);
    }

    #[test]
    fn the_digest_tells_fields_apart() {
        let a = digest_of(None, "Ilica 1", Some("2"), "Zagreb", None, None, "HR");
        let b = digest_of(None, "Ilica 12", None, "Zagreb", None, None, "HR");
        assert_ne!(a, b);
        assert_eq!(a.len(), 64);
    }

    #[test]
    fn a_suggestion_is_normalised_like_a_set_address() {
        let s = normalised(SuggestedAddress {
            company: None,
            line1: "1600 Amphitheatre Pkwy".to_string(),
            line2: None,
            city: "Mountain View".to_string(),
            province: Some("California".to_string()),
            postal_code: Some("94043-1351".to_string()),
            country: "us".to_string(),
        });
        assert_eq!(s.province.as_deref(), Some("CA"));
        assert_eq!(s.country, "US");
    }
}
