//! Where an app answers an extension point (design note 0029, §3, "Registering"): the
//! one seam every flow that calls apps (payments, search) looks endpoints up through.
//!
//! `hypha_apps::find_endpoint` decides: an app installed through OAuth answers where
//! its manifest's `extension_points` say (URLs resolved against its `app_url`), and
//! only while its grant has the point's scope (`ExtensionPoint::SCOPE`: `payments.*`
//! need `payments:write`, `search.query` `products:read`, `shipping.rates`
//! `shipping:read`; §4.1 step 5); an app made by hand where staff set
//! (`PUT /admin/apps/{key}/extension-points`, `app_extension_endpoints`). What personal
//! data a call may carry is the app's grant too (`redaction`).

use std::time::Duration;

use hypha_apps::{AppKey, HasApps, HasExtensionEndpoints, KnownPoint, open_secret};
use hypha_core::{AppPoints, RepoError};
use hypha_extensions::Endpoint;

use crate::{redact::Redaction, state::HostEnv};

/// An app's endpoint for a point, ready to call: its URL, its secret in plain text, its
/// own timeout (`None`: the point's), and the personal data its grant lets it receive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedEndpoint {
    pub app: AppKey,
    pub url: String,
    pub secret: String,
    pub timeout: Option<Duration>,
    pub redaction: Redaction,
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

/// Where an app answers `point`, as `hypha_apps::find_endpoint` decides; `None` when it
/// doesn't (missing, disabled, uninstalled, not granted the point's scope, no endpoint,
/// or a point this build doesn't know). No `Send` bound, like every port.
#[allow(async_fn_in_trait)]
pub trait AppEndpoints {
    async fn endpoint(
        &self,
        app: &AppKey,
        point: &'static str,
    ) -> Result<Option<ResolvedEndpoint>, EndpointError>;
}

/// `AppEndpoints` over a request's ports: the app (its secret opened with the host's
/// key) and its endpoint for the point.
pub struct HostAppEndpoints<'a, P> {
    pub ports: &'a P,
    pub env: &'a HostEnv,
}

/// The point named `name` as `hypha_apps` takes it, when this build knows it.
pub fn known(name: &str) -> Option<KnownPoint> {
    hypha_extensions::known_point(name).map(|k| KnownPoint {
        name: k.name,
        version: k.version,
        scope: k.scope,
    })
}

impl<P: HasApps + HasExtensionEndpoints> AppEndpoints for HostAppEndpoints<'_, P> {
    async fn endpoint(
        &self,
        app: &AppKey,
        point: &'static str,
    ) -> Result<Option<ResolvedEndpoint>, EndpointError> {
        let Some(point) = known(point) else {
            return Ok(None);
        };
        let Some((found, registered)) = hypha_apps::find_endpoint(self.ports, app, point).await?
        else {
            return Ok(None);
        };
        let secret =
            open_secret(self.env, &found).map_err(|e| EndpointError::Secret(e.to_string()))?;
        Ok(Some(ResolvedEndpoint {
            app: app.clone(),
            url: registered.url.as_str().to_string(),
            secret,
            timeout: None,
            redaction: Redaction::for_app(&found),
        }))
    }
}

impl<P: HasApps + HasExtensionEndpoints> AppPoints for HostAppEndpoints<'_, P> {
    async fn answers(&self, app: &AppKey, point: &str) -> Result<bool, RepoError> {
        let Some(point) = known(point) else {
            return Ok(false);
        };
        Ok(hypha_apps::find_endpoint(self.ports, app, point)
            .await?
            .is_some())
    }
}
