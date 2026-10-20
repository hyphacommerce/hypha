//! The host's payment providers (design note 0029, §3.1, §10 item 5): the ones it was
//! built with (`Storage::Payments`, `manual`), and every installed app answering
//! `payments.start` under its own key (`app_stripe`). An app's start, capture, void and
//! refund are its extension points, called through `call::<P>` (signed, capped, parsed
//! and recorded in its call log with the payment), failing closed: an app that gives no
//! usable answer is `app_unavailable`.

use hypha_apps::{HasApps, HasExtensionEndpoints};
use hypha_core::{AppKey, AppPoints, RepoError, optional_text};
use hypha_extensions::{
    ExtensionError, ExtensionPoint, HasExtensionCalls, PaymentActionRequest, PaymentActionResponse,
    PaymentStartRequest, PaymentsCapture, PaymentsRefund, PaymentsStart, PaymentsVoid, call,
};
use hypha_payments::{
    ActionAnswer, ActionRequest, NextAction, PaymentAction, PaymentError, PaymentProviders,
    PaymentRequest, ProviderKey, Started, no_settings,
};

use crate::{
    app_endpoints::{AppEndpoints, HostAppEndpoints},
    extensions::HostExtensions,
    state::HostEnv,
};

/// One request's payment providers: the built-in ones, and the store's payment apps.
pub struct HostPayments<'a, P, B> {
    /// What the host was built with (`Storage::payment_providers`).
    pub built_in: &'a B,
    pub ports: &'a P,
    pub endpoints: HostAppEndpoints<'a, P>,
    pub caller: HostExtensions<'a>,
    pub env: &'a HostEnv,
}

impl<P, B> HostPayments<'_, P, B>
where
    P: HasApps + HasExtensionEndpoints + HasExtensionCalls + hypha_apps::HasSporeSettings,
{
    /// Whether `key` is a spore's provider whose spore staff have disabled (design note
    /// 0030, §3): it then reports unavailable and collects nothing, as a disabled app's.
    /// The spore is the one the key belongs to (`spore_stripe` or `spore_stripe_…`).
    async fn spore_disabled(&self, key: &ProviderKey) -> Result<bool, RepoError> {
        if key.is_manual() || key.app().is_some() {
            return Ok(false);
        }
        Ok(!hypha_apps::spore_enabled(self.ports.apps(), key.as_str()).await?)
    }

    /// A spore provider's store settings (design note 0030, §8): its spore's, secret
    /// fields opened, for the provider (`PaymentRequest::store_settings`); empty before
    /// the merchant saves any. `None` while the spore is off or not set up (it then
    /// collects nothing, as a disabled app).
    async fn spore_settings(
        &self,
        key: &ProviderKey,
    ) -> Result<Option<serde_json::Map<String, serde_json::Value>>, PaymentError> {
        let failed = |reason: String| PaymentError::ProviderFailed {
            provider: key.clone(),
            reason,
        };
        let spore = hypha_apps::spore_of(self.ports.apps(), key.as_str())
            .await
            .map_err(|e| failed(e.to_string()))?;
        let Some(spore) = spore.filter(|app| app.enabled()) else {
            return Ok(None);
        };
        let settings =
            hypha_apps::read_spore_settings(self.ports, self.env.secret_box(), spore.key())
                .await
                .map_err(|e| failed(e.to_string()))?;
        Ok(Some(settings.unwrap_or_default()))
    }

    /// Where `app` answers `point`, when it's installed, enabled and does.
    async fn endpoint(
        &self,
        key: &ProviderKey,
        app: &AppKey,
        point: &'static str,
    ) -> Result<Option<crate::app_endpoints::ResolvedEndpoint>, PaymentError> {
        self.endpoints
            .endpoint(app, point)
            .await
            .map_err(|e| PaymentError::ProviderFailed {
                provider: key.clone(),
                reason: e.to_string(),
            })
    }

    /// Captures, voids or refunds at the point `Pt`.
    async fn act_at<Pt>(
        &self,
        key: &ProviderKey,
        app: &AppKey,
        request: &ActionRequest,
    ) -> Result<ActionAnswer, PaymentError>
    where
        Pt: ExtensionPoint<Request = PaymentActionRequest, Response = PaymentActionResponse>,
    {
        // An app that doesn't answer the point can't take the action.
        let endpoint = self.endpoint(key, app, Pt::NAME).await?.ok_or_else(|| {
            PaymentError::ActionUnsupported {
                provider: key.clone(),
                action: request.action,
            }
        })?;
        let body = PaymentActionRequest {
            payment: request.payment,
            order: request.order,
            amount: request.amount,
            reference: request.reference.clone(),
        };
        let correlation = request.payment.to_string();
        let answer = call::<Pt>(
            &self.caller,
            self.ports.extension_calls(),
            self.env,
            &endpoint.as_endpoint(),
            &body,
            &request.idempotency_key,
            Some(&correlation),
        )
        .await
        .map_err(unavailable)?;
        Ok(match answer {
            PaymentActionResponse::Accepted { reference } => ActionAnswer::Accepted {
                reference: checked_reference(app, Pt::NAME, reference)?,
            },
            PaymentActionResponse::Refused { reason } => ActionAnswer::Refused { reason },
        })
    }
}

impl<P, B> PaymentProviders for HostPayments<'_, P, B>
where
    P: HasApps + HasExtensionEndpoints + HasExtensionCalls + hypha_apps::HasSporeSettings,
    B: PaymentProviders,
{
    /// The built-in ones'; apps are found when asked.
    fn keys(&self) -> Vec<ProviderKey> {
        self.built_in.keys()
    }

    async fn available(&self, key: &ProviderKey) -> Result<bool, RepoError> {
        match key.app() {
            Some(app) => self.endpoints.answers(&app, PaymentsStart::NAME).await,
            None if self.spore_disabled(key).await? => Ok(false),
            None => self.built_in.available(key).await,
        }
    }

    fn check_settings(
        &self,
        key: &ProviderKey,
        settings: &serde_json::Value,
    ) -> Result<(), PaymentError> {
        match key.app() {
            // An app keeps its own settings (design note 0029, §4.2).
            Some(_) => no_settings(key, settings),
            None => self.built_in.check_settings(key, settings),
        }
    }

    async fn start(
        &self,
        key: &ProviderKey,
        payment: &PaymentRequest,
    ) -> Result<Started, PaymentError> {
        let Some(app) = key.app() else {
            if key.is_manual() {
                return self.built_in.start(key, payment).await;
            }
            let Some(settings) = self.spore_settings(key).await? else {
                return Err(PaymentError::UnknownProvider(key.clone()));
            };
            let request = PaymentRequest {
                store_settings: settings,
                ..payment.clone()
            };
            return self.built_in.start(key, &request).await;
        };
        let endpoint = self
            .endpoint(key, &app, PaymentsStart::NAME)
            .await?
            .ok_or_else(|| PaymentError::UnknownProvider(key.clone()))?;
        let request = PaymentStartRequest {
            payment: payment.payment,
            order: payment.order,
            order_number: payment.order_number,
            method: payment.method,
            amount: payment.amount,
            channel: payment.channel,
            market: payment.market,
            country: payment.country,
            return_url: payment.return_url.clone(),
            // Personal data goes to an app only when its grant has
            // `customer_data:email` (design note 0029, AP10).
            customer_email: if endpoint.redaction.removes_email() {
                None
            } else {
                Some(payment.customer_email.clone())
            },
        };
        // The payment's ID: a retried start (the shopper asking again) must not
        // charge twice.
        let key_text = payment.payment.to_string();
        let answer = call::<PaymentsStart>(
            &self.caller,
            self.ports.extension_calls(),
            self.env,
            &endpoint.as_endpoint(),
            &request,
            &key_text,
            Some(&key_text),
        )
        .await
        .map_err(unavailable)?;
        Ok(Started {
            next_action: match answer.next_action {
                hypha_extensions::NextAction::Redirect { url } => NextAction::Redirect { url },
                hypha_extensions::NextAction::ClientWidget { element, props } => {
                    NextAction::ClientWidget { element, props }
                }
                hypha_extensions::NextAction::Instructions { text } => {
                    NextAction::Instructions(text)
                }
            },
            reference: checked_reference(&app, PaymentsStart::NAME, answer.reference)?,
            // An app's reports name their attempt themselves, when they do.
            attempt: None,
        })
    }

    async fn act(
        &self,
        key: &ProviderKey,
        request: &ActionRequest,
    ) -> Result<ActionAnswer, PaymentError> {
        let Some(app) = key.app() else {
            if key.is_manual() {
                return self.built_in.act(key, request).await;
            }
            let Some(settings) = self.spore_settings(key).await? else {
                return Err(PaymentError::ActionUnsupported {
                    provider: key.clone(),
                    action: request.action,
                });
            };
            let request = ActionRequest {
                store_settings: settings,
                ..request.clone()
            };
            return self.built_in.act(key, &request).await;
        };
        match request.action {
            PaymentAction::Capture => self.act_at::<PaymentsCapture>(key, &app, request).await,
            PaymentAction::Void => self.act_at::<PaymentsVoid>(key, &app, request).await,
            PaymentAction::Refund => self.act_at::<PaymentsRefund>(key, &app, request).await,
        }
    }
}

/// A point's failure as payments take it: `app_unavailable`, with the app and point.
fn unavailable(e: ExtensionError) -> PaymentError {
    PaymentError::AppUnavailable {
        app: e.app().clone(),
        point: e.point(),
        reason: e.to_string(),
    }
}

/// The provider's reference as a payment keeps it (short text); one that isn't is an
/// answer the payment can't take, so the app is unavailable as if it hadn't parsed.
fn checked_reference(
    app: &AppKey,
    point: &'static str,
    reference: Option<String>,
) -> Result<Option<String>, PaymentError> {
    optional_text("reference", reference).map_err(|e| PaymentError::AppUnavailable {
        app: app.clone(),
        point,
        reason: e.to_string(),
    })
}
