//! Spores in the HTTP layer: the host's capabilities as a spore's environment
//! (`spore_host`), and spores' routes (`spore_routes!`, `gate`).

use std::sync::Arc;

use axum::{
    extract::{FromRequestParts, Request},
    handler::Handler,
    http::request::Parts,
    response::{IntoResponse, Response},
};
use hypha_core::AppKey;

use hypha_spore_core::{HttpError, HttpMethod, HttpRequest, HttpResponse, SporeHost, SporeHttp};

use crate::{
    client_ip::ClientIp,
    error::ApiError,
    extract::{Admin, Caller, Storefront},
    host::HostCalls,
    outbound::{CallError, Method, OutboundCall, OutboundPolicy, check_literal},
    rate_limit::Rule,
    state::{AppState, HostEnv, SharedState, Storage},
};

/// The host's capabilities for its spores (`hypha_spore_core::SporeHost`): its clock,
/// IDs, randomness and `SecretBox` (`env`), and HTTP through `calls`, the client calls
/// to apps go through, under the same `policy` (natively only public addresses unless
/// `outbound.allow_private`; no redirects; the answer capped).
pub fn spore_host(env: &HostEnv, calls: HostCalls, policy: OutboundPolicy) -> SporeHost {
    let runtime = env.runtime();
    SporeHost {
        clock: runtime.clock.shared(),
        ids: Arc::new(runtime.ids.clone()),
        random: Arc::new(runtime.random.clone()),
        secrets: Arc::new(env.secret_box().clone()),
        http: SporeHttp::new(move |request| {
            let calls = calls.clone();
            Box::pin(async move { send(&calls, policy, request).await })
        }),
    }
}

/// Sends a spore's request through `calls`.
async fn send(
    calls: &HostCalls,
    policy: OutboundPolicy,
    request: HttpRequest,
) -> Result<HttpResponse, HttpError> {
    check_literal(policy, &request.url).map_err(http_error)?;
    let call = OutboundCall {
        method: match request.method {
            HttpMethod::Get => Method::Get,
            HttpMethod::Post => Method::Post,
            HttpMethod::Put => Method::Put,
            HttpMethod::Patch => Method::Patch,
            HttpMethod::Delete => Method::Delete,
        },
        url: request.url,
        timeout: request.timeout,
        headers: request.headers,
        body: request.body,
        discard_answer: false,
        max_answer: request.max_answer,
        purpose: crate::outbound::CallPurpose::Other,
    };
    let answer = calls.send(call).await.map_err(http_error)?;
    Ok(HttpResponse {
        status: answer.status,
        content_type: answer.content_type,
        body: answer.body,
    })
}

fn http_error(e: CallError) -> HttpError {
    match e {
        CallError::Timeout => HttpError::Timeout,
        CallError::TooLarge => HttpError::TooLarge,
        CallError::Failed(reason) => HttpError::Failed(reason),
    }
}

// ---------- routes ----------

/// Which surface a spore's route is on, by its prefix: `public` (`/spores/<name>/…`, no
/// credentials, rate limited per client address by `Rule::SporePublic`), `admin`
/// (`/admin/spores/<name>/…`, a `Caller<Admin>`) or `storefront`
/// (`/storefront/spores/<name>/…`, a `Caller<Storefront>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SporeSurface {
    Public,
    Admin,
    Storefront,
}

impl SporeSurface {
    /// The prefix before the spore's name.
    pub const fn prefix(self) -> &'static str {
        match self {
            SporeSurface::Public => "/spores/",
            SporeSurface::Admin => "/admin/spores/",
            SporeSurface::Storefront => "/storefront/spores/",
        }
    }
}

/// Whether `path` is under `surface`'s prefix for the spore `name`
/// (`/admin/spores/<name>/…`): what `spore_routes!` checks when the spore builds, so a
/// spore can't shadow a core route or another spore's. Core keeps out of these prefixes
/// (a test).
pub const fn route_fits(surface: SporeSurface, path: &str, name: &str) -> bool {
    let prefix = surface.prefix().as_bytes();
    let path = path.as_bytes();
    let name = name.as_bytes();
    let whole = prefix.len() + name.len() + 1;
    if path.len() <= whole {
        return false;
    }
    let mut i = 0;
    while i < prefix.len() {
        if path[i] != prefix[i] {
            return false;
        }
        i += 1;
    }
    let mut j = 0;
    while j < name.len() {
        if path[prefix.len() + j] != name[j] {
            return false;
        }
        j += 1;
    }
    path[whole - 1] == b'/'
}

/// Runs a spore's handler behind what every spore route gets (`spore_routes!` wraps
/// each in this): a public route's rate limit by client address (`Rule::SporePublic`,
/// before anything is read); an admin or storefront route's credentials, checked first
/// like core's (a stranger gets the 401, and learns nothing of the spore); then
/// `403 app_disabled` while staff have the spore turned off. The handler authenticates
/// again for its own ports.
pub async fn gate<S, H, T>(
    state: SharedState<S>,
    request: Request,
    surface: SporeSurface,
    name: &'static str,
    handler: H,
) -> Response
where
    S: Storage,
    H: Handler<T, SharedState<S>>,
    T: 'static,
{
    let (mut parts, body) = request.into_parts();
    if let Err(e) = admit(&state, &mut parts, surface, name).await {
        return e.into_response();
    }
    handler.call(Request::from_parts(parts, body), state).await
}

async fn admit<S: Storage>(
    state: &SharedState<S>,
    parts: &mut Parts,
    surface: SporeSurface,
    name: &'static str,
) -> Result<(), ApiError> {
    use hypha_apps::HasApps;
    let key = AppKey::of_spore(name).map_err(ApiError::internal)?;
    let enabled = match surface {
        SporeSurface::Public => {
            let ClientIp(ip) = ClientIp::from_request_parts(parts, state).await?;
            crate::rate_limit::enforce(&state.limiter, Rule::SporePublic, &ip).await?;
            spore_enabled(state, &key).await?
        }
        SporeSurface::Admin => {
            let caller = Caller::<Admin>::from_request_parts(parts, state).await?;
            let (ports, _) = caller.authenticate(state).await?;
            hypha_apps::spore_enabled(ports.apps(), key.as_str())
                .await
                .map_err(ApiError::internal)?
        }
        SporeSurface::Storefront => {
            let caller = Caller::<Storefront>::from_request_parts(parts, state).await?;
            let (ports, _) = caller.authenticate(state).await?;
            hypha_apps::spore_enabled(ports.apps(), key.as_str())
                .await
                .map_err(ApiError::internal)?
        }
    };
    match enabled {
        true => Ok(()),
        false => Err(ApiError::app_disabled(&key)),
    }
}

/// The built spore `T` this host runs, for its route handlers: `let stripe =
/// spore::<Stripe, S>(&state)?;`.
pub fn spore<T: hypha_spore_core::Spore, S: Storage>(state: &AppState<S>) -> Result<&T, ApiError> {
    hypha_spore_core::spore::<T, S::Ports<'static>>(state.storage.spores())
        .ok_or_else(|| ApiError::internal(format!("spore {:?} isn't built in", T::NAME)))
}

/// Whether the spore `key` is enabled now (its app, made at startup; a missing one is
/// off).
pub async fn spore_enabled<S: Storage>(
    state: &AppState<S>,
    key: &AppKey,
) -> Result<bool, ApiError> {
    use hypha_apps::HasApps;
    let ports = state.public_ports().await?;
    hypha_apps::spore_enabled(ports.apps(), key.as_str())
        .await
        .map_err(ApiError::internal)
}

/// `public`, `admin` or `storefront` as a `SporeSurface` (`spore_routes!`'s rows).
#[doc(hidden)]
#[macro_export]
macro_rules! __spore_surface {
    (public) => {
        $crate::spores::SporeSurface::Public
    };
    (admin) => {
        $crate::spores::SporeSurface::Admin
    };
    (storefront) => {
        $crate::spores::SporeSurface::Storefront
    };
}

/// The OpenAPI document a spore's route is in: admin routes and public ones (a
/// provider's webhooks, for the operator) in the admin document, storefront ones in the
/// storefront's.
#[doc(hidden)]
#[macro_export]
macro_rules! __spore_doc {
    (storefront) => {
        $crate::routes::openapi::Doc::Storefront
    };
    ($other:ident) => {
        $crate::routes::openapi::Doc::Admin
    };
}

/// A spore's routes, in `route_table!`'s shape. Invoke it at your crate's root; handler paths are from
/// there (`module::handler`):
///
/// ```ignore
/// hypha_spore::spore_routes! { crate::stripe_routes for Stripe:
///     [public, POST, "/spores/stripe/webhook", webhook::receive],
///     [admin, GET, "/admin/spores/stripe/payments/{id}", admin::payment],
///     [storefront, GET, "/storefront/spores/stripe/elements/payment.js", elements::payment],
/// }
/// ```
///
/// Paths must be under the surface's prefix and the spore's name (`/spores/<name>/`,
/// `/admin/spores/<name>/`, `/storefront/spores/<name>/`): checked when the spore
/// builds. Handlers are generic over the storage (`S: Storage`) and annotated with
/// `#[utoipa::path]` like core's (method and path as here; the host's `openapi` command
/// documents them under the spore's tag). It defines the macro `stripe_routes!` at the
/// crate root, which a host built with the spore expands with its concrete storage, for
/// the reason `router!` exists: `host! { spores: [Stripe], routes: [hypha_spore_stripe::stripe_routes] }`.
/// Every route answers `403 app_disabled` while staff have the spore disabled, and
/// public ones are rate limited per client address (`Rule::SporePublic`).
#[macro_export]
macro_rules! spore_routes {
    ($k:tt :: $name:ident for $spore:ty :
        $([$surface:ident, $method:ident, $path:literal, $module:ident :: $func:ident]),* $(,)?) => {
        /// The spore whose routes these are (`spore_routes!`).
        #[doc(hidden)]
        #[allow(non_upper_case_globals)]
        pub const $name: &str = <$spore as $crate::__private::Spore>::NAME;

        const _: () = {
            $(
                assert!(
                    $crate::spores::route_fits($crate::__spore_surface!($surface), $path, $name),
                    concat!(
                        "spore route ", $path, " isn't under its surface's prefix and the spore's name ",
                        "(/spores/<name>/, /admin/spores/<name>/, /storefront/spores/<name>/)"
                    ),
                );
            )*
        };

        $crate::__spore_routes! { ($) $k $name
            $([$surface, $method, $path, $module :: $func])*
        }
    };
}

/// `spore_routes!`'s second step: `$d` is a `$` and `$k` the spore crate's `crate`, so
/// the macro it defines names the spore's items as `$crate::…` from wherever it's
/// expanded.
#[doc(hidden)]
#[macro_export]
macro_rules! __spore_routes {
    (($d:tt) $k:tt $name:ident $([$surface:ident, $method:ident, $path:literal, $module:ident :: $func:ident])*) => {
        /// This spore's routes for a host's concrete storage (`router <storage>`), and
        /// their OpenAPI rows (`rows`): what `host!` and `worker!` expand.
        #[macro_export]
        macro_rules! $name {
            (router $d storage:ty) => {
                $crate::__private::Router::<$crate::state::SharedState<$d storage>>::new()
                $(
                    .route(
                        $path,
                        $crate::routes::on(
                            $crate::routes::MethodFilter::$method,
                            |$crate::__private::State(state): $crate::__private::State<
                                $crate::state::SharedState<$d storage>,
                            >,
                             request: $crate::__private::Request| async move {
                                $crate::spores::gate(
                                    state,
                                    request,
                                    $crate::__spore_surface!($surface),
                                    $d $k :: $name,
                                    $d $k :: $module :: $func :: <$d storage>,
                                )
                                .await
                            },
                        ),
                    )
                )*
            };
            (rows) => {
                vec![$(
                    $crate::__private::pastey::paste! {
                        $crate::routes::openapi::Row {
                            doc: $crate::__spore_doc!($surface),
                            method: stringify!($method),
                            path: $path,
                            operation_id: format!(
                                "{}.spore_{}.{}",
                                stringify!($surface),
                                $d $k :: $name,
                                stringify!($func)
                            ),
                            tag: format!("spore_{}", $d $k :: $name),
                            public: stringify!($surface) == "public",
                            annotated_path: <$d $k :: $module :: [<__path_ $func>] as $crate::__private::utoipa::Path>::path,
                            annotated_methods: <$d $k :: $module :: [<__path_ $func>] as $crate::__private::utoipa::Path>::methods,
                            operation: <$d $k :: $module :: [<__path_ $func>] as $crate::__private::utoipa::Path>::operation,
                            schemas: <$d $k :: $module :: [<__path_ $func>] as $crate::__private::utoipa::__dev::SchemaReferences>::schemas,
                        }
                    }
                ),*]
            };
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_route_fits_under_its_surfaces_prefix_and_its_spores_name() {
        use SporeSurface::{Admin, Public, Storefront};
        assert!(route_fits(Public, "/spores/stripe/webhook", "stripe"));
        assert!(route_fits(
            Admin,
            "/admin/spores/stripe/payments/{id}",
            "stripe"
        ));
        assert!(route_fits(
            Storefront,
            "/storefront/spores/stripe/x.js",
            "stripe"
        ));
        for (surface, path) in [
            (Public, "/spores/stripe"),
            (Public, "/spores/stripe/"),
            (Public, "/spores/stripey/webhook"),
            (Public, "/admin/spores/stripe/x"),
            (Admin, "/spores/stripe/x"),
            (Admin, "/admin/products"),
            (Storefront, "/storefront/spores/strip/x"),
        ] {
            assert!(!route_fits(surface, path, "stripe"), "{surface:?} {path}");
        }
    }

    /// So a spore's routes can never shadow core's.
    #[test]
    fn no_core_route_is_under_a_spore_prefix() {
        for row in crate::routes::openapi::rows() {
            for surface in [
                SporeSurface::Public,
                SporeSurface::Admin,
                SporeSurface::Storefront,
            ] {
                assert!(!row.path.starts_with(surface.prefix()), "{}", row.path);
            }
        }
    }
}
