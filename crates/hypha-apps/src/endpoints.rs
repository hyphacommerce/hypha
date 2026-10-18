//! Where an app implements extension points (design note 0029, §3, "Registering"):
//! one URL per point it answers, with the version it speaks. Staff set them for a
//! private app (one without a manifest); an app installed from its manifest gets them
//! from it. Which app a flow calls is per-object configuration (a payment method's
//! provider, the store's search app); this is where Hypha finds the URL.

use std::fmt;

use hypha_core::{AppKey, CallbackUrl, Ctx, RepoError, Scope};

use crate::{AppError, AppRepo, CommandError, HasApps};

/// One point an app answers: `payments.start` v1 at its URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionEndpoint {
    /// The point's name, as the extensions document lists it (`payments.start`).
    pub point: String,
    pub version: u16,
    pub url: CallbackUrl,
}

/// A point and version this build knows (`hypha_extensions`' catalog), as the host
/// passes them: this crate doesn't depend on the catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KnownPoint {
    pub name: &'static str,
    pub version: u16,
}

impl fmt::Display for KnownPoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} v{}", self.name, self.version)
    }
}

/// Uniqueness and reference rules the endpoint repo enforces.
pub mod keys {
    /// An endpoint needs its app: `RepoError::Reference(ENDPOINT_APP)` otherwise.
    pub const ENDPOINT_APP: &str = "extension_endpoint_app";
}

#[allow(async_fn_in_trait)]
pub trait ExtensionEndpointRepo {
    /// `app`'s endpoints, by point name.
    async fn list_for_app(&self, app: &AppKey) -> Result<Vec<ExtensionEndpoint>, RepoError>;
    /// Where `app` answers `point`, if it does.
    async fn get(&self, app: &AppKey, point: &str) -> Result<Option<ExtensionEndpoint>, RepoError>;
    /// Makes `endpoints` `app`'s endpoints, all at once (none removes them all). The
    /// caller makes sure each point appears once. Refused with
    /// `Reference(keys::ENDPOINT_APP)` when the app doesn't exist; an app's endpoints
    /// go when it does.
    async fn replace_for_app(
        &self,
        app: &AppKey,
        endpoints: &[ExtensionEndpoint],
    ) -> Result<(), RepoError>;
}

/// Whatever holds the `ExtensionEndpointRepo`: a host's per-request ports, a test world.
pub trait HasExtensionEndpoints {
    type ExtensionEndpoints: ExtensionEndpointRepo;
    fn extension_endpoints(&self) -> &Self::ExtensionEndpoints;
}

/// Borrowed ports are ports too.
impl<T: HasExtensionEndpoints + ?Sized> HasExtensionEndpoints for &T {
    type ExtensionEndpoints = T::ExtensionEndpoints;
    fn extension_endpoints(&self) -> &Self::ExtensionEndpoints {
        (**self).extension_endpoints()
    }
}

/// An app's endpoints (`apps:read`); `app_not_found` for an app that doesn't exist.
pub async fn list_extension_endpoints<S: HasApps + HasExtensionEndpoints>(
    ctx: &Ctx,
    ports: &S,
    key: &str,
) -> Result<Vec<ExtensionEndpoint>, CommandError> {
    ctx.require(Scope::AppsRead)?;
    let app = existing(ports, key).await?;
    Ok(ports.extension_endpoints().list_for_app(&app).await?)
}

/// An endpoint as a request gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointInput {
    pub point: String,
    pub version: u16,
    pub url: String,
}

/// Replaces an app's endpoints (`apps:write`): each a point and version in `known`
/// (`unknown_extension_point` otherwise), named once (`duplicate_extension_point`), at
/// an `https://` URL (or `http://` to localhost, like webhooks). An empty list removes
/// them, and the app is no longer called at any point.
pub async fn set_extension_endpoints<S: HasApps + HasExtensionEndpoints>(
    ctx: &Ctx,
    ports: &S,
    key: &str,
    inputs: Vec<EndpointInput>,
    known: &[KnownPoint],
) -> Result<Vec<ExtensionEndpoint>, CommandError> {
    ctx.require(Scope::AppsWrite)?;
    let app = existing(ports, key).await?;
    let mut endpoints: Vec<ExtensionEndpoint> = Vec::with_capacity(inputs.len());
    for input in inputs {
        if !known
            .iter()
            .any(|k| k.name == input.point && k.version == input.version)
        {
            return Err(AppError::UnknownExtensionPoint {
                point: input.point,
                version: input.version,
            }
            .into());
        }
        if endpoints.iter().any(|e| e.point == input.point) {
            return Err(AppError::DuplicateExtensionPoint(input.point).into());
        }
        endpoints.push(ExtensionEndpoint {
            url: CallbackUrl::new(&input.url).map_err(AppError::from)?,
            point: input.point,
            version: input.version,
        });
    }
    endpoints.sort_by(|a, b| a.point.cmp(&b.point));
    match ports
        .extension_endpoints()
        .replace_for_app(&app, &endpoints)
        .await
    {
        // Deleted since it was read.
        Err(RepoError::Reference(keys::ENDPOINT_APP)) => {
            Err(CommandError::AppNotFound(key.to_string()))
        }
        done => {
            done?;
            Ok(endpoints)
        }
    }
}

async fn existing<S: HasApps>(ports: &S, key: &str) -> Result<AppKey, CommandError> {
    let not_found = || CommandError::AppNotFound(key.to_string());
    let app = AppKey::new(key).map_err(|_| not_found())?;
    ports.apps().get(&app).await?.ok_or_else(not_found)?;
    Ok(app)
}
