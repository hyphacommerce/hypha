//! The host's `RateProvider` (design note 0015, section 5): `app_*` keys go to external
//! apps over a signed HTTP callback, `spore_*` keys to the spores this host was built
//! with. Static dispatch, a branch per kind, so futures stay provably `Send`.
//!
//! The callback protocol (JSON shapes, signature) lives here for now; when a second
//! host (Workers) needs it, it moves into an adapter crate behind the `HttpClient` port,
//! like `hypha-store-s3`.

use std::time::Duration;

use hmac::{Hmac, KeyInit, Mac};
use hypha_core::{Clock, Money, Price, VariantId};
use hypha_shipping::{
    AppProvider, AppProviderRepo, DeliveryEstimate, ProviderError, ProviderKey, ProviderKind, Rate,
    RateProvider, RateRequest, ServiceInfo, open_secret,
};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::{
    routes::{address::PostalAddressResponse, rfc3339},
    state::{HostEnv, SystemClock},
};

/// The header carrying a callback's signature: `t=<unix seconds>,v1=<hex HMAC-SHA256 of
/// "<t>.<body>" under the provider's secret>` (Stripe's scheme; webhooks reuse it).
pub const SIGNATURE_HEADER: &str = "Hypha-Signature";
/// The ID of one rate request, for the app's logs and de-duplication.
pub const REQUEST_ID_HEADER: &str = "Hypha-Request-Id";

/// One `RateProvider` per request: the store's app providers, the shared HTTP client,
/// and the host's key to open their secrets.
pub struct HostRates<'a, A: AppProviderRepo> {
    pub apps: &'a A,
    pub http: &'a reqwest::Client,
    pub env: &'a HostEnv,
    pub clock: &'a SystemClock,
}

impl<A: AppProviderRepo> RateProvider for HostRates<'_, A> {
    async fn services(&self, key: &ProviderKey) -> Result<Vec<ServiceInfo>, ProviderError> {
        match key.kind() {
            ProviderKind::App => {
                let app = self.app(key).await?;
                if !app.services().is_empty() {
                    return Ok(app.services().to_vec());
                }
                let url = format!(
                    "{}/services",
                    app.callback_url().as_str().trim_end_matches('/')
                );
                let body = self
                    .call(&app, reqwest::Method::GET, &url, Vec::new())
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
                let app = self.app(key).await?;
                let id = format!("rreq_{}", uuid::Uuid::now_v7().simple());
                let payload = CallbackRequest {
                    id: id.clone(),
                    sent_at: rfc3339(self.clock.now()).map_err(|_| failed(key, "bad clock"))?,
                    request: WireRateRequest::from(request),
                };
                let body = serde_json::to_vec(&payload).map_err(|e| failed(key, e.to_string()))?;
                let answer = self
                    .call(
                        &app,
                        reqwest::Method::POST,
                        app.callback_url().as_str(),
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

impl<A: AppProviderRepo> HostRates<'_, A> {
    async fn app(&self, key: &ProviderKey) -> Result<AppProvider, ProviderError> {
        let app = self
            .apps
            .get(key)
            .await
            .map_err(|e| failed(key, e.to_string()))?
            .ok_or_else(|| ProviderError::Unknown(key.clone()))?;
        if !app.enabled() {
            return Err(ProviderError::Disabled(key.clone()));
        }
        Ok(app)
    }

    /// Sends a signed request within the app's timeout; the body of a 2xx answer.
    async fn call(
        &self,
        app: &AppProvider,
        method: reqwest::Method,
        url: &str,
        body: Vec<u8>,
    ) -> Result<Vec<u8>, ProviderError> {
        let key = app.key();
        let secret = open_secret(self.env, app).map_err(|e| failed(key, e.to_string()))?;
        let at = self.clock.now().unix_micros() / 1_000_000;
        let signature = sign(&secret, at, &body);
        let response = self
            .http
            .request(method, url)
            .timeout(Duration::from_millis(u64::from(app.timeout_ms())))
            .header("content-type", "application/json")
            .header(SIGNATURE_HEADER, signature)
            .header(REQUEST_ID_HEADER, uuid::Uuid::now_v7().simple().to_string())
            .body(body)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    ProviderError::Timeout(key.clone())
                } else {
                    failed(key, format!("unreachable: {e}"))
                }
            })?;
        let status = response.status();
        if !status.is_success() {
            return Err(failed(key, format!("answered HTTP {}", status.as_u16())));
        }
        let bytes = response.bytes().await.map_err(|e| {
            if e.is_timeout() {
                ProviderError::Timeout(key.clone())
            } else {
                failed(key, e.to_string())
            }
        })?;
        Ok(bytes.to_vec())
    }
}

/// `t=<at>,v1=<hex HMAC-SHA256 of "<at>.<body>">`.
pub fn sign(secret: &str, at: i64, body: &[u8]) -> String {
    // HMAC takes keys of any length.
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .unwrap_or_else(|_| unreachable!("HMAC accepts every key length"));
    mac.update(at.to_string().as_bytes());
    mac.update(b".");
    mac.update(body);
    let digest = mac.finalize().into_bytes();
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("t={at},v1={hex}")
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
    use super::*;

    #[test]
    fn a_signature_is_the_hmac_of_time_dot_body() {
        // Checked against `printf '1700000000.{}' | openssl dgst -sha256 -hmac 'hypha_shs_test'`.
        assert_eq!(
            sign("hypha_shs_test", 1_700_000_000, b"{}"),
            "t=1700000000,v1=7e9b80e5d75d36d3052ca2dc364618d7dca90cfe78476758f403d6388b8173c7"
        );
    }
}
