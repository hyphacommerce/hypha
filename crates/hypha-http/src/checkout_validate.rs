//! The host's `CheckoutValidators`: every installed app answering `checkout.validate`
//! (`hypha_apps::find_endpoint`'s rule: enabled, granted `carts:read`), as the store's
//! settings treat it, asked through the point with every call in its call log. What each
//! app is sent is redacted for its grant, and the digest of that is what its kept answer
//! is matched by.

use std::sync::OnceLock;

use hypha_apps::{
    App, AppKey, AppRepo, ExtensionEndpointRepo, HasApps, HasExtensionEndpoints, endpoint_of,
};
use hypha_cart::{LineStatus, PricedCart};
use hypha_checkout::{CheckoutValidators, Validator};
use hypha_checkout_rules::{
    CheckoutSettings, CheckoutSettingsRepo, HasCheckoutSettings, ValidationAppSetting,
    room_for_validation_app,
};
use hypha_core::{Language, Message, MessageSource, RepoError, ValidationStep};
use hypha_extensions::{
    CheckoutValidate, CheckoutValidateRequest, ExtensionPoint, HasExtensionCalls, PostalAddress,
    ValidatedAddressCheck, ValidatedCart, ValidatedLine, ValidatedShippingGroup, call,
};
use sha2::{Digest, Sha256};

use crate::{
    app_endpoints::{AppEndpoints, EndpointError, HostAppEndpoints, known},
    error::ApiError,
    extensions::HostExtensions,
    redact::Redaction,
    state::HostEnv,
};

/// One request's validation apps: the store's settings and apps, the call log, the shared
/// HTTP client.
pub struct HostValidators<'a, P> {
    pub ports: &'a P,
    pub endpoints: HostAppEndpoints<'a, P>,
    pub caller: HostExtensions<'a>,
    pub env: &'a HostEnv,
    /// The apps answering the point, with their settings: read once per request, though
    /// a request asks for its cart's validators more than once (refreshing them, then the
    /// response's messages).
    pub asked: OnceLock<Vec<(App, ValidationAppSetting)>>,
}

impl<P> HostValidators<'_, P>
where
    P: HasCheckoutSettings + HasApps + HasExtensionEndpoints,
{
    async fn asked(&self) -> Result<&[(App, ValidationAppSetting)], RepoError> {
        if let Some(found) = self.asked.get() {
            return Ok(found);
        }
        let apps = answering(self.ports).await?;
        let settings = if apps.is_empty() {
            CheckoutSettings::default()
        } else {
            self.ports
                .checkout_settings()
                .get()
                .await?
                .unwrap_or_default()
        };
        let found = apps
            .into_iter()
            .map(|app| {
                let setting = settings.validation_app(app.key());
                (app, setting)
            })
            .collect();
        Ok(self.asked.get_or_init(|| found))
    }
}

impl<P> CheckoutValidators for HostValidators<'_, P>
where
    P: HasCheckoutSettings + HasApps + HasExtensionEndpoints + HasExtensionCalls,
{
    type Body = ValidatedCart;

    async fn validators(
        &self,
        cart: &PricedCart,
        language: Option<&Language>,
    ) -> Result<Vec<Validator<ValidatedCart>>, RepoError> {
        Ok(self
            .asked()
            .await?
            .iter()
            .filter(|(_, setting)| setting.enabled)
            .map(|(app, setting)| {
                let body = validated_cart(cart, Redaction::for_app(app), language);
                Validator {
                    app: app.key().clone(),
                    block_on_failure: setting.block_on_failure,
                    digest: digest(&body),
                    body,
                }
            })
            .collect())
    }

    async fn ask(
        &self,
        validator: &Validator<ValidatedCart>,
        step: ValidationStep,
    ) -> Result<Vec<Message>, String> {
        let endpoint = match self
            .endpoints
            .endpoint(&validator.app, CheckoutValidate::NAME)
            .await
        {
            Ok(Some(found)) => found,
            Ok(None) => return Err("it doesn't answer checkout.validate (any more)".to_string()),
            Err(e @ (EndpointError::Repo(_) | EndpointError::Secret(_))) => {
                return Err(e.to_string());
            }
        };
        let request = CheckoutValidateRequest {
            step,
            cart: validator.body.clone(),
        };
        // The same cart at the same step is the same question: an app may answer a retry
        // from its cache.
        let digest = &validator.digest;
        let key = format!(
            "validate_{}_{}",
            step.as_str(),
            &digest[..digest.len().min(32)]
        );
        let correlation = validator.body.id.to_string();
        let answer = call::<CheckoutValidate>(
            &self.caller,
            self.ports.extension_calls(),
            self.env,
            &endpoint.as_endpoint(),
            &request,
            &key,
            Some(&correlation),
        )
        .await
        .map_err(|e| {
            tracing::warn!(app = %validator.app, point = CheckoutValidate::NAME, why = %e,
                "a validation app couldn't be asked");
            e.to_string()
        })?;
        answer
            .messages
            .into_iter()
            .map(|m| {
                Message::new(
                    m.severity,
                    m.code,
                    m.path,
                    &m.message,
                    MessageSource::App {
                        app: validator.app.clone(),
                    },
                )
                .map_err(|e| e.to_string())
            })
            .collect()
    }
}

/// The apps answering `checkout.validate` now, by key: `endpoint_of`'s rule, with the
/// hand-made apps' endpoints read in one query rather than one per app.
async fn answering<P: HasApps + HasExtensionEndpoints>(ports: &P) -> Result<Vec<App>, RepoError> {
    let Some(point) = known(CheckoutValidate::NAME) else {
        return Ok(Vec::new());
    };
    let apps = ports.apps().list().await?;
    let set = ports
        .extension_endpoints()
        .list_for_point(point.name)
        .await?;
    let mut found: Vec<App> = apps
        .into_iter()
        .filter(|app| {
            if !app.may_answer(point.scope) {
                return false;
            }
            match app.manifest_endpoints() {
                Some(registered) => registered
                    .iter()
                    .any(|e| e.point == point.name && e.version == point.version),
                None => set
                    .iter()
                    .any(|(key, e)| key == app.key() && e.version == point.version),
            }
        })
        .collect();
    found.sort_by(|a, b| a.key().as_str().cmp(b.key().as_str()));
    Ok(found)
}

/// Every app answering `checkout.validate` now, with how `settings` treat it.
pub async fn validation_apps<P: HasApps + HasExtensionEndpoints>(
    ports: &P,
    settings: &CheckoutSettings,
) -> Result<Vec<ValidationAppSetting>, RepoError> {
    Ok(answering(ports)
        .await?
        .iter()
        .map(|app| settings.validation_app(app.key()))
        .collect())
}

/// Refuses `app` starting to answer `checkout.validate` when as many others do as a store
/// may have (`too_many_validation_apps`); an app answering already keeps its place.
pub async fn ensure_room_for<P: HasApps + HasExtensionEndpoints>(
    ports: &P,
    app: &AppKey,
) -> Result<(), ApiError> {
    let apps = answering(ports).await?;
    if apps.iter().any(|a| a.key() == app) {
        return Ok(());
    }
    room_for_validation_app(apps.len())?;
    Ok(())
}

/// Refuses approving `request` when it would make its app answer `checkout.validate` (its
/// manifest registers the point, and the grant has `carts:read`) while the store has as
/// many such apps as it may (`too_many_validation_apps`). A request that isn't found is
/// left for the approval to refuse.
pub async fn ensure_room_for_request<P>(
    ports: &P,
    request: hypha_core::OAuthRequestId,
) -> Result<(), ApiError>
where
    P: HasApps + HasExtensionEndpoints + hypha_oauth::HasOAuthRequests,
{
    use hypha_oauth::OAuthRequestRepo;
    let Some(found) = ports.oauth_requests().get(request).await? else {
        return Ok(());
    };
    let (Some(key), Some(manifest)) = (found.app(), found.manifest()) else {
        return Ok(());
    };
    let registers = manifest
        .document()
        .extension_points
        .iter()
        .any(|p| p.point == CheckoutValidate::NAME && p.version == CheckoutValidate::VERSION);
    if !registers {
        return Ok(());
    }
    let granted_now = ports
        .apps()
        .get(key)
        .await?
        .and_then(|app| app.grant().map(|g| g.has(CheckoutValidate::SCOPE)))
        .unwrap_or(false);
    let asked_for = found.scopes().contains(&CheckoutValidate::SCOPE)
        || CheckoutValidate::SCOPE
            .implied_by()
            .is_some_and(|wider| found.scopes().contains(&wider));
    if !(granted_now || asked_for) {
        return Ok(());
    }
    ensure_room_for(ports, key).await
}

/// Refuses enabling `app` when it would then answer `checkout.validate` while the store
/// has as many apps doing so as it may (`too_many_validation_apps`). An app enabled
/// already, or that wouldn't answer, passes.
pub async fn ensure_room_for_enabling<P: HasApps + HasExtensionEndpoints>(
    ports: &P,
    app: &AppKey,
) -> Result<(), ApiError> {
    let Some(mut found) = ports.apps().get(app).await? else {
        return Ok(());
    };
    if found.enabled() {
        return Ok(());
    }
    found.set_enabled(true);
    ensure_room_for_app(ports, &found).await
}

/// Refuses `app` as it's about to be stored (a manifest refreshed, an app enabled) when it
/// would answer `checkout.validate` while the store has as many apps doing so as it may.
pub async fn ensure_room_for_app<P: HasApps + HasExtensionEndpoints>(
    ports: &P,
    app: &App,
) -> Result<(), ApiError> {
    let Some(point) = known(CheckoutValidate::NAME) else {
        return Ok(());
    };
    if endpoint_of(ports, app, point).await?.is_none() {
        return Ok(());
    }
    ensure_room_for(ports, app.key()).await
}

/// The cart as `checkout.validate` sends it, with `redaction`'s personal fields left out,
/// telling the app the shopper reads `language` (`ContentLanguages::told`).
pub fn validated_cart(
    cart: &PricedCart,
    redaction: Redaction,
    language: Option<&Language>,
) -> ValidatedCart {
    let shipping_address = cart.shipping_address.as_ref().map(|a| {
        let mut address = PostalAddress::from(a);
        redaction.postal_address(&mut address);
        address
    });
    let removes_email = redaction.removes_email() && cart.email.is_some();
    ValidatedCart {
        id: cart.id,
        market: cart.market,
        channel: cart.channel,
        country: cart.country,
        currency: cart.currency,
        customer: cart.customer,
        email: cart
            .email
            .as_ref()
            .filter(|_| !removes_email)
            .map(|e| e.as_str().to_string()),
        shipping_address,
        shipping_address_check: cart
            .address_check
            .as_ref()
            .filter(|_| cart.shipping_address.is_some())
            .map(|v| ValidatedAddressCheck {
                verdict: v.verdict,
                flags: v.flags,
            }),
        lines: cart
            .lines
            .iter()
            .map(|l| ValidatedLine {
                variant: l.variant,
                product: l.product,
                sku: l.details.as_ref().and_then(|d| d.sku.clone()),
                quantity: l.quantity.value(),
                unit_price: match &l.status {
                    LineStatus::Priced { unit_price, .. } => Some(unit_price.money()),
                    LineStatus::Unavailable(_) => None,
                },
                weight_grams: l
                    .details
                    .as_ref()
                    .and_then(|d| d.shipping_weight)
                    .map(|w| w.grams()),
            })
            .collect(),
        shipping_groups: cart
            .shipping_groups
            .iter()
            .map(|g| ValidatedShippingGroup {
                option: g.selected.as_ref().map(|s| s.option),
                code: g.selected.as_ref().map(|s| s.code.clone()),
                provider: g
                    .selected
                    .as_ref()
                    .and_then(|s| s.quote.as_ref())
                    .map(|q| q.provider.as_str().to_string()),
            })
            .collect(),
        subtotal: cart.subtotal,
        total: cart.total,
        redacted: removes_email.then(|| vec!["email".to_string()]),
        language: language.cloned(),
    }
}

/// SHA-256 of the body as sent, hex.
fn digest(body: &ValidatedCart) -> String {
    // Serialising plain data can't fail; an empty digest would only mean asking again.
    let bytes = serde_json::to_vec(body).unwrap_or_default();
    Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkout_rules_names_the_point_this_build_calls() {
        assert_eq!(hypha_checkout_rules::VALIDATE_POINT, CheckoutValidate::NAME);
    }
}
