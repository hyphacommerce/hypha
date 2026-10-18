//! Where an app answers an extension point (design note 0029, §3, "Registering"): the
//! one seam every flow that calls apps (payments, search) looks endpoints up through.
//!
//! Today the registrations are the app's `app_extension_endpoints` rows, set by staff
//! (`PUT /admin/apps/{key}/extension-points`) for private apps. When apps are installed
//! from manifests (§4), `HostAppEndpoints` reads the manifest's registrations instead,
//! filtered by the app's grant (§4.1 step 5: a point needing a scope the grant lacks
//! isn't called); the callers don't change.

use std::time::Duration;

use hypha_apps::{
    App, AppKey, AppRepo, ExtensionEndpointRepo, HasApps, HasExtensionEndpoints, open_secret,
};
use hypha_core::{AppPoints, RepoError};
use hypha_extensions::Endpoint;

use crate::state::HostEnv;

/// An app's endpoint for a point, ready to call: its URL, its secret in plain text, and
/// its own timeout (`None`: the point's).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedEndpoint {
    pub app: AppKey,
    pub url: String,
    pub secret: String,
    pub timeout: Option<Duration>,
}

impl ResolvedEndpoint {
    /// As `hypha_extensions::call` takes it.
    pub fn as_endpoint(&self) -> Endpoint<'_> {
        Endpoint {
            app: &self.app,
            url: &self.url,
            secret: &self.secret,
            timeout: self.timeout,
            headers: &[],
        }
    }
}

/// Why an endpoint couldn't be read: storage, or a secret that won't open (a changed
/// `HYPHA_SECRET_KEY`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EndpointError {
    Repo(RepoError),
    Secret(String),
}

impl std::fmt::Display for EndpointError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EndpointError::Repo(e) => write!(f, "{e}"),
            EndpointError::Secret(e) => write!(f, "the app's secret won't open: {e}"),
        }
    }
}

impl From<RepoError> for EndpointError {
    fn from(e: RepoError) -> Self {
        EndpointError::Repo(e)
    }
}

/// Where an installed, enabled app answers `point`; `None` when the app is missing,
/// disabled, or doesn't answer it. No `Send` bound, like every port.
#[allow(async_fn_in_trait)]
pub trait AppEndpoints {
    async fn endpoint(
        &self,
        app: &AppKey,
        point: &'static str,
    ) -> Result<Option<ResolvedEndpoint>, EndpointError>;
}

/// `AppEndpoints` over a request's ports: the app (enabled, its secret opened with the
/// host's key) and its registered endpoint.
pub struct HostAppEndpoints<'a, P> {
    pub ports: &'a P,
    pub env: &'a HostEnv,
}

impl<P: HasApps + HasExtensionEndpoints> HostAppEndpoints<'_, P> {
    /// The app, when it's installed and enabled.
    async fn enabled(&self, app: &AppKey) -> Result<Option<App>, RepoError> {
        Ok(self.ports.apps().get(app).await?.filter(App::enabled))
    }
}

impl<P: HasApps + HasExtensionEndpoints> AppEndpoints for HostAppEndpoints<'_, P> {
    async fn endpoint(
        &self,
        app: &AppKey,
        point: &'static str,
    ) -> Result<Option<ResolvedEndpoint>, EndpointError> {
        let Some(found) = self.enabled(app).await? else {
            return Ok(None);
        };
        let Some(registered) = self.ports.extension_endpoints().get(app, point).await? else {
            return Ok(None);
        };
        let secret =
            open_secret(self.env, &found).map_err(|e| EndpointError::Secret(e.to_string()))?;
        Ok(Some(ResolvedEndpoint {
            app: app.clone(),
            url: registered.url.as_str().to_string(),
            secret,
            timeout: None,
        }))
    }
}

impl<P: HasApps + HasExtensionEndpoints> AppPoints for HostAppEndpoints<'_, P> {
    async fn answers(&self, app: &AppKey, point: &str) -> Result<bool, RepoError> {
        if self.enabled(app).await?.is_none() {
            return Ok(false);
        }
        Ok(self
            .ports
            .extension_endpoints()
            .get(app, point)
            .await?
            .is_some())
    }
}
