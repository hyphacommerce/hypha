//! The host's `RateProvider` (design note 0015, section 5): `app_*` keys go to external
//! apps over a signed HTTP callback, `spore_*` keys to the spores this host was built
//! with. Static dispatch, a branch per kind, so futures stay provably `Send`.
//!
//! The callback protocol (JSON shapes, signature) lives here, shared by the hosts; each
//! sends the calls through its `HostCalls` (design note 0026).

use std::time::Duration;

use hypha_apps::{App, AppKey, AppRepo, HasApps, open_secret};
use hypha_core::{Clock, Money, Price, VariantId};
use hypha_shipping::{
    AppProvider, AppProviderRepo, DeliveryEstimate, HasAppProviders, ProviderError, ProviderKey,
    ProviderKind, Rate, RateProvider, RateRequest, ServiceInfo,
};
use serde::{Deserialize, Serialize};

use crate::{
    host::{HostCalls, HostClock, HostIds},
    outbound::{CallError, Method, OutboundPolicy, SignedCall},
    routes::{address::PostalAddressResponse, rfc3339},
    state::HostEnv,
};

/// The ID of one rate request (`rreq_…`, the body's `id` too), for the app's logs and
/// de-duplication.
pub const REQUEST_ID_HEADER: &str = "Hypha-Request-Id";

/// One `RateProvider` per request: the store's apps and their rate settings, the shared
/// HTTP client, and the host's key to open the apps' secrets.
pub struct HostRates<'a, P: HasAppProviders + HasApps> {
    pub ports: &'a P,
    pub calls: &'a HostCalls,
    pub outbound: OutboundPolicy,
    pub env: &'a HostEnv,
    pub clock: &'a HostClock,
    /// For rate request IDs (`rreq_…`).
    pub ids: &'a HostIds,
}

impl<P: HasAppProviders + HasApps> RateProvider for HostRates<'_, P> {
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
                let body = self
                    .call(
                        &provider,
                        &app,
                        Method::Get,
                        &url,
                        &request_id(self.ids),
                        Vec::new(),
                    )
                    .await?;
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
                let id = request_id(self.ids);
                let payload = CallbackRequest {
                    id: id.clone(),
                    sent_at: rfc3339(self.clock.now()).map_err(|_| failed(key, "bad clock"))?,
                    request: WireRateRequest::from(request),
                };
                let body = serde_json::to_vec(&payload).map_err(|e| failed(key, e.to_string()))?;
                let answer = self
                    .call(
                        &provider,
                        &app,
                        Method::Post,
                        provider.callback_url().as_str(),
                        &id,
                        body,
                    )
                    .await?;
                let parsed: RatesResponse = serde_json::from_slice(&answer)
                    .map_err(|e| failed(key, format!("unreadable rates: {e}")))?;
                parsed.rates.into_iter().map(|r| r.into_rate(key)).collect()
            }
            ProviderKind::Spore => Err(ProviderError::Unknown(key.clone())),
        }
    }
}

impl<P: HasAppProviders + HasApps> HostRates<'_, P> {
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

    /// Sends a signed request within the provider's timeout; the body of a 2xx answer.
    async fn call(
        &self,
        provider: &AppProvider,
        app: &App,
        method: Method,
        url: &str,
        id: &str,
        body: Vec<u8>,
    ) -> Result<Vec<u8>, ProviderError> {
        let key = provider.key();
        let secret = open_secret(self.env, app).map_err(|e| failed(key, e.to_string()))?;
        let call = SignedCall {
            method,
            url,
            timeout: Duration::from_millis(u64::from(provider.timeout_ms())),
            secret: &secret,
            at: self.clock.now().unix_micros() / 1_000_000,
            headers: &[(REQUEST_ID_HEADER, id.to_string())],
            body,
            discard_answer: false,
        };
        let sent = self.calls.send_signed(self.outbound, call).await;
        sent.map_err(|e| match e {
            CallError::Timeout => ProviderError::Timeout(key.clone()),
            CallError::Failed(reason) => failed(key, reason),
        })
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

// ---------- the callback's JSON (a published contract, SH6) ----------

#[derive(Serialize)]
struct CallbackRequest {
    id: String,
    /// RFC 3339, UTC.
    sent_at: String,
    request: WireRateRequest,
}

#[derive(Serialize)]
struct WireRateRequest {
    currency: hypha_core::Currency,
    origin: Option<PostalAddressResponse>,
    destination: PostalAddressResponse,
    items: Vec<WireRateItem>,
    subtotal: Money,
}

#[derive(Serialize)]
struct WireRateItem {
    variant: VariantId,
    sku: Option<String>,
    quantity: u32,
    unit_price: Money,
    weight_grams: Option<u32>,
}

impl From<&RateRequest> for WireRateRequest {
    fn from(r: &RateRequest) -> Self {
        Self {
            currency: r.currency,
            origin: r.origin.as_ref().map(PostalAddressResponse::from),
            destination: PostalAddressResponse::from(&r.destination),
            items: r
                .items
                .iter()
                .map(|i| WireRateItem {
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
}

#[derive(Deserialize)]
struct RatesResponse {
    rates: Vec<WireRate>,
}

#[derive(Deserialize)]
struct WireRate {
    service: String,
    name: String,
    amount: Money,
    #[serde(default)]
    estimate: Option<WireEstimate>,
    #[serde(default)]
    reference: Option<String>,
}

#[derive(Deserialize)]
struct WireEstimate {
    min_days: u16,
    max_days: u16,
}

impl WireRate {
    /// A rate as the domain takes it: never negative, a sane estimate.
    fn into_rate(self, key: &ProviderKey) -> Result<Rate, ProviderError> {
        let amount = Price::new(self.amount)
            .map_err(|e| failed(key, format!("rate for {:?}: {e}", self.service)))?;
        let estimate = self
            .estimate
            .map(|e| DeliveryEstimate::new(e.min_days, e.max_days))
            .transpose()
            .map_err(|e| failed(key, e.to_string()))?;
        Ok(Rate {
            service: self.service,
            name: self.name,
            amount,
            estimate,
            reference: self.reference,
        })
    }
}

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
