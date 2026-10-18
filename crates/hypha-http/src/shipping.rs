//! The host's `RateProvider` (design note 0015, section 5): `app_*` keys go to external
//! apps through the extension point `shipping.rates` v1 (design note 0029, AP9; the
//! callback's body unchanged on the wire, and every call recorded in the call log),
//! `spore_*` keys to the spores this host was built with. Static dispatch, a branch per
//! kind, so futures stay provably `Send`.
//!
//! The rate request's JSON is `hypha_extensions::shipping`'s; listing a provider's
//! services (`GET <callback_url>/services`, admin-side, not an extension point) stays
//! here. Each host sends the calls through its `HostCalls` (design note 0026).

use std::time::Duration;

use hypha_apps::{App, AppKey, AppRepo, HasApps, open_secret};
use hypha_core::{CartId, Clock, Price};
use hypha_extensions::{
    Endpoint, ExtensionError, HasExtensionCalls, PostalAddress, QuotedRate, RateCallbackRequest,
    RateItemBody, RateRequestBody, ShippingRates, call,
};
use hypha_shipping::{
    AppProvider, AppProviderRepo, DeliveryEstimate, HasAppProviders, ProviderError, ProviderKey,
    ProviderKind, Rate, RateProvider, RateRequest, ServiceInfo,
};
use serde::Deserialize;

use crate::{
    extensions::HostExtensions,
    host::{HostCalls, HostClock, HostIds},
    outbound::{CallError, Method, OutboundPolicy, SignedCall},
    routes::rfc3339,
    state::HostEnv,
};

/// The ID of one rate request (`rreq_…`, the body's `id` too), for the app's logs and
/// de-duplication. Sent besides `Idempotency-Key`, which carries the same ID, since
/// rate apps were written against it (0015 §5).
pub const REQUEST_ID_HEADER: &str = "Hypha-Request-Id";

/// One `RateProvider` per request: the store's apps and their rate settings, the call
/// log, the shared HTTP client, and the host's key to open the apps' secrets.
pub struct HostRates<'a, P: HasAppProviders + HasApps + HasExtensionCalls> {
    pub ports: &'a P,
    pub calls: &'a HostCalls,
    pub outbound: OutboundPolicy,
    pub env: &'a HostEnv,
    pub clock: &'a HostClock,
    /// For rate request IDs (`rreq_…`).
    pub ids: &'a HostIds,
    /// The store the calls are for (`Hypha-Store`).
    pub store: hypha_core::StoreId,
    /// The cart the rates are for, recorded with each call; `None` for the admin's.
    pub cart: Option<CartId>,
}

impl<P: HasAppProviders + HasApps + HasExtensionCalls> RateProvider for HostRates<'_, P> {
    async fn services(&self, key: &ProviderKey) -> Result<Vec<ServiceInfo>, ProviderError> {
        match key.kind() {
            ProviderKind::App => {
                let (provider, app) = self.app(key).await?;
                if !provider.services().is_empty() {
                    return Ok(provider.services().to_vec());
                }
                let url = format!(
                    "{}/services",
                    provider.callback_url().as_str().trim_end_matches('/')
                );
                let body = self.services_call(&provider, &app, &url).await?;
                let parsed: ServicesResponse = serde_json::from_slice(&body)
                    .map_err(|e| failed(key, format!("unreadable services: {e}")))?;
                Ok(parsed
                    .services
                    .into_iter()
                    .map(|s| ServiceInfo {
                        code: s.code,
                        name: s.name,
                    })
                    .collect())
            }
            // A host built with spores routes their keys here, one arm each.
            ProviderKind::Spore => Err(ProviderError::Unknown(key.clone())),
        }
    }

    async fn quote(
        &self,
        key: &ProviderKey,
        request: &RateRequest,
    ) -> Result<Vec<Rate>, ProviderError> {
        match key.kind() {
            ProviderKind::App => {
                let (provider, app) = self.app(key).await?;
                let secret = open_secret(self.env, &app).map_err(|e| failed(key, e.to_string()))?;
                let id = request_id(self.ids);
                let payload = RateCallbackRequest {
                    id: id.clone(),
                    sent_at: rfc3339(self.clock.now()).map_err(|_| failed(key, "bad clock"))?,
                    request: wire_request(request),
                };
                let headers = [(REQUEST_ID_HEADER, id.clone())];
                let endpoint = Endpoint {
                    app: app.key(),
                    url: provider.callback_url().as_str(),
                    secret: &secret,
                    timeout: Some(Duration::from_millis(u64::from(provider.timeout_ms()))),
                    headers: &headers,
                };
                let correlation = self.cart.map(|c| c.to_string());
                let answer = call::<ShippingRates>(
                    &self.caller(),
                    self.ports.extension_calls(),
                    self.env,
                    &endpoint,
                    &payload,
                    &id,
                    correlation.as_deref(),
                )
                .await
                .map_err(|e| provider_error(key, e))?;
                answer
                    .rates
                    .into_iter()
                    .map(|r| into_rate(key, r))
                    .collect()
            }
            ProviderKind::Spore => Err(ProviderError::Unknown(key.clone())),
        }
    }
}

impl<P: HasAppProviders + HasApps + HasExtensionCalls> HostRates<'_, P> {
    /// The rate calls' caller: the host's calls, for this store.
    fn caller(&self) -> HostExtensions<'_> {
        HostExtensions {
            calls: self.calls,
            outbound: self.outbound,
            clock: self.clock,
            store: self.store,
        }
    }

    /// The app's rate settings and the app itself (its secret, whether it's enabled).
    async fn app(&self, key: &ProviderKey) -> Result<(AppProvider, App), ProviderError> {
        let provider = self
            .ports
            .app_providers()
            .get(key)
            .await
            .map_err(|e| failed(key, e.to_string()))?
            .ok_or_else(|| ProviderError::Unknown(key.clone()))?;
        let app_key = AppKey::new(key.as_str()).map_err(|_| ProviderError::Unknown(key.clone()))?;
        let app = self
            .ports
            .apps()
            .get(&app_key)
            .await
            .map_err(|e| failed(key, e.to_string()))?
            .ok_or_else(|| ProviderError::Unknown(key.clone()))?;
        if !app.enabled() {
            return Err(ProviderError::Disabled(key.clone()));
        }
        Ok((provider, app))
    }

    /// Asks for the provider's services: a signed `GET` within its timeout (the body
    /// empty, so the signed string is `"<t>."`); the body of a 2xx answer.
    async fn services_call(
        &self,
        provider: &AppProvider,
        app: &App,
        url: &str,
    ) -> Result<Vec<u8>, ProviderError> {
        let key = provider.key();
        let secret = open_secret(self.env, app).map_err(|e| failed(key, e.to_string()))?;
        let call = SignedCall {
            method: Method::Get,
            url,
            timeout: Duration::from_millis(u64::from(provider.timeout_ms())),
            secret: &secret,
            at: self.clock.now().unix_micros() / 1_000_000,
            headers: &[(REQUEST_ID_HEADER, request_id(self.ids))],
            body: Vec::new(),
            discard_answer: false,
        };
        let sent = self.calls.send_signed(self.outbound, call).await;
        sent.map_err(|e| match e {
            CallError::Timeout => ProviderError::Timeout(key.clone()),
            other => failed(key, other.reason()),
        })
    }
}

/// A failed rate call as shipping takes it: its options are left out with
/// `provider_failed` (the point's fallback, SH7).
fn provider_error(key: &ProviderKey, e: ExtensionError) -> ProviderError {
    match e {
        ExtensionError::Timeout { .. } => ProviderError::Timeout(key.clone()),
        ExtensionError::Unavailable { reason, .. } | ExtensionError::Invalid { reason, .. } => {
            failed(key, reason)
        }
    }
}

/// A new rate request ID: `rreq_` and a UUIDv7, in the header and the body alike.
fn request_id(ids: &HostIds) -> String {
    use hypha_core::IdGenerator;
    format!("rreq_{}", ids.new_uuid().simple())
}

fn failed(key: &ProviderKey, reason: impl Into<String>) -> ProviderError {
    ProviderError::Failed {
        provider: key.clone(),
        reason: reason.into(),
    }
}

/// A rate request as the point sends it.
fn wire_request(r: &RateRequest) -> RateRequestBody {
    RateRequestBody {
        currency: r.currency,
        origin: r.origin.as_ref().map(PostalAddress::from),
        destination: PostalAddress::from(&r.destination),
        items: r
            .items
            .iter()
            .map(|i| RateItemBody {
                variant: i.variant,
                sku: i.sku.clone(),
                quantity: i.quantity.value(),
                unit_price: i.unit_price.money(),
                weight_grams: i.weight.map(|w| w.grams()),
            })
            .collect(),
        subtotal: r.subtotal,
    }
}

/// A rate as the domain takes it. `ShippingRates::check` refused negative amounts and
/// backwards estimates already; the constructors check again, as they would any input.
fn into_rate(key: &ProviderKey, rate: QuotedRate) -> Result<Rate, ProviderError> {
    let amount = Price::new(rate.amount)
        .map_err(|e| failed(key, format!("rate for {:?}: {e}", rate.service)))?;
    let estimate = rate
        .estimate
        .map(|e| DeliveryEstimate::new(e.min_days, e.max_days))
        .transpose()
        .map_err(|e| failed(key, e.to_string()))?;
    Ok(Rate {
        service: rate.service,
        name: rate.name,
        amount,
        estimate,
        reference: rate.reference,
    })
}

// ---------- the services answer (0015 §5) ----------

#[derive(Deserialize)]
struct ServicesResponse {
    services: Vec<WireService>,
}

#[derive(Deserialize)]
struct WireService {
    code: String,
    name: String,
}

#[cfg(test)]
mod tests {
    use crate::outbound::sign;

    #[test]
    fn a_signature_is_the_hmac_of_time_dot_body() {
        // Checked against `printf '1700000000.{}' | openssl dgst -sha256 -hmac 'hypha_shs_test'`.
        assert_eq!(
            sign("hypha_shs_test", 1_700_000_000, b"{}"),
            "t=1700000000,v1=7e9b80e5d75d36d3052ca2dc364618d7dca90cfe78476758f403d6388b8173c7"
        );
    }
}
