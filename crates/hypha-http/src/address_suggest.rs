//! Address autocomplete through the store's autocomplete app (`CheckoutSettings`), asked
//! through `address.suggest` v1 and `address.resolve` v1 with every call in its call log
//! (without bodies: what someone types isn't kept). Both points fall back: suggestions
//! that fail are none, and the person types the address.

use hypha_apps::{HasApps, HasExtensionEndpoints};
use hypha_checkout_rules::{CheckoutSettingsRepo, HasCheckoutSettings};
use hypha_core::{AppKey, IdGenerator, RepoError, SuggestedAddress};
use hypha_extensions::{
    AddressResolve, AddressResolveRequest, AddressSuggest, AddressSuggestRequest,
    AddressSuggestion, Answered, ExtensionError, ExtensionPoint, HasExtensionCalls, apply_policy,
    call,
};

use crate::{
    app_endpoints::{AppEndpoints, EndpointError, HostAppEndpoints},
    extensions::HostExtensions,
    state::HostEnv,
};

/// One request's autocomplete: the store's settings and apps, the call log, the shared
/// HTTP client.
pub struct HostAutocomplete<'a, P> {
    pub ports: &'a P,
    pub endpoints: HostAppEndpoints<'a, P>,
    pub caller: HostExtensions<'a>,
    pub env: &'a HostEnv,
}

impl<P> HostAutocomplete<'_, P>
where
    P: HasCheckoutSettings + HasApps + HasExtensionEndpoints + HasExtensionCalls,
{
    /// The store's autocomplete app, when one is named.
    pub async fn app(&self) -> Result<Option<AppKey>, RepoError> {
        let settings = self.ports.checkout_settings().get().await?;
        Ok(settings.and_then(|s| s.autocomplete_app().cloned()))
    }

    /// The app's suggestions, or none when it fails (logged: the call log has why).
    pub async fn suggest(
        &self,
        app: &AppKey,
        request: &AddressSuggestRequest,
    ) -> Result<Vec<AddressSuggestion>, RepoError> {
        match self.ask::<AddressSuggest>(app, request, "suggest").await? {
            Ok(answer) => Ok(answer.suggestions),
            Err(why) => {
                tracing::warn!(%app, point = AddressSuggest::NAME, %why,
                    "address suggestions failed; none are offered");
                Ok(Vec::new())
            }
        }
    }

    /// The picked suggestion's address, or why the app couldn't give it (`502
    /// app_unavailable`: the person types the address instead).
    pub async fn resolve(
        &self,
        app: &AppKey,
        request: &AddressResolveRequest,
    ) -> Result<Result<SuggestedAddress, ExtensionError>, RepoError> {
        Ok(self
            .ask::<AddressResolve>(app, request, "resolve")
            .await?
            .map(|answer| answer.address))
    }

    /// One call to `app`'s endpoint for `X`. A storage failure is the caller's; anything
    /// else is the app's, answered as the point's error.
    async fn ask<X: ExtensionPoint>(
        &self,
        app: &AppKey,
        request: &X::Request,
        key_prefix: &str,
    ) -> Result<Result<X::Response, ExtensionError>, RepoError> {
        let unavailable = |reason: String| ExtensionError::Unavailable {
            app: app.clone(),
            point: X::NAME,
            reason,
        };
        let endpoint = match self.endpoints.endpoint(app, X::NAME).await {
            Ok(Some(found)) => found,
            Ok(None) => {
                return Ok(Err(unavailable(format!(
                    "it doesn't answer {} (any more)",
                    X::NAME
                ))));
            }
            Err(EndpointError::Repo(e)) => return Err(e),
            Err(e @ EndpointError::Secret(_)) => return Ok(Err(unavailable(e.to_string()))),
        };
        // Nothing changes: a fresh key per call, for the app's logs.
        let key = format!("{key_prefix}_{}", self.env.new_uuid().simple());
        let answered = apply_policy::<X>(
            call::<X>(
                &self.caller,
                self.ports.extension_calls(),
                self.env,
                &endpoint.as_endpoint(),
                request,
                &key,
                None,
            )
            .await,
        );
        Ok(match answered {
            Ok(Answered::App(answer)) => Ok(answer),
            Ok(Answered::Fallback(e)) | Err(e) => Err(e),
        })
    }
}
