//! Hypha's native host as a library plus a macro: a binary is `hypha_host_axum::host! {
//! spores: [...] }`, which expands, for the spores listed, the deployment's type
//! (`Deployment`), its router and its dispatch job, and a `main` that runs `start::run`.
//! Our own binary (`main.rs`) lists none, so the stock build is what it always was; a
//! deployment with spores is a small crate `hypha deploy` generates.
//!
//! Why a macro rather than a generic `run::<Spores>()`: axum must prove every handler's
//! future `Send`, and with our `Send`-free repository and provider traits the compiler
//! can only do that for concrete types (the reason `hypha_http::router!` exists). So
//! what touches the spores' providers and handlers across an `.await` (the router, the
//! dispatch job) is expanded in the caller's crate with the concrete spore types; the
//! rest (startup, serving, the other jobs) is generic over `Deployment`.

use hypha_core::EventHandlers;
use hypha_http::host::BoxFuture;
use hypha_http::jobs::Step;
use hypha_http::state::SharedState;
use hypha_spore_core::SporeSet;
use hypha_store_postgres::{CORE, MigrationSet};

mod calls;
mod email;
#[cfg(test)]
mod handlers_test;
mod limiter;
mod platform;
// A spore for the route tests, and its routes, declared at the crate root as
// `spore_routes!` asks (and before `route_tests`, which names the macro it defines).
#[cfg(test)]
mod test_spore;
#[cfg(test)]
hypha_http::spore_routes! { crate::test_spore_routes for crate::test_spore::TestSpore:
    [public, GET, "/spores/test/hello", test_spore::hello],
    [admin, GET, "/admin/spores/test/settings", test_spore::settings],
    [storefront, GET, "/storefront/spores/test/greeting", test_spore::storefront_greeting],
}
#[cfg(test)]
mod route_tests;
mod runner;
mod start;
mod storage;

pub use hypha_http::handlers::CoreHandlers;
pub use hypha_http::payments::CoreProviders;
pub use start::run;
pub use storage::{HandlersOf, PgDatabase, PgPorts, PgStorage, PooledClient, ProvidersOf};

/// A deployment: the spores a host is built with, and what must be expanded for them
/// with concrete types (see the crate's docs). Written by `deployment!` (and `host!`),
/// never by hand.
pub trait Deployment: Sized + 'static {
    /// `()` for none, or a tuple of spores (`hypha_spore_core::SporeSet`). Their
    /// handlers must run on this host's ports.
    type Spores: SporeSet<Handlers: EventHandlers<PgPorts>>
        + hypha_spore_core::SporeRuntime<PgPorts>;

    /// `hypha_http::router!` for this deployment's storage, with its spores' routes
    /// (`spore_routes!`).
    fn router() -> axum::Router<SharedState<PgStorage<Self>>>;

    /// Its spores' routes as OpenAPI rows (the `openapi` command documents them).
    fn spore_rows() -> Vec<hypha_http::routes::openapi::Row>;

    /// One dispatch step (`hypha_http::jobs::dispatch`), which runs the handlers.
    fn dispatch(state: SharedState<PgStorage<Self>>) -> BoxFuture<Step>;
}

/// Core's migrations, then each spore's, in the order listed.
pub fn migrations<D: Deployment>() -> Vec<MigrationSet> {
    let mut sets = vec![CORE];
    sets.extend(<D::Spores as SporeSet>::migrations());
    sets
}

/// Routes listed for a spore the build doesn't have (`routes: [...]` naming another
/// spore's macro than the `spores` listed): what's wrong, one line each.
pub fn check_routes<D: Deployment>() -> Vec<String> {
    let names: Vec<String> = <D::Spores as SporeSet>::registrations()
        .iter()
        .map(|s| format!("spore_{}", s.name))
        .collect();
    let mut problems: Vec<String> = D::spore_rows()
        .iter()
        .filter(|row| !names.contains(&row.tag))
        .map(|row| {
            format!(
                "route {} {} is {}'s, a spore this build doesn't list",
                row.method, row.path, row.tag
            )
        })
        .collect();
    problems.dedup();
    problems
}

/// What the macros' expansions name, so a deployment crate needs no dependency but
/// this one.
#[doc(hidden)]
pub mod __private {
    pub use axum::Router;
    pub use hypha_http;
    pub use hypha_http::host::BoxFuture;
    pub use hypha_http::jobs::Step;
    pub use hypha_http::state::SharedState;
}

/// Declares a deployment: an uninhabited type implementing `Deployment` for `spores`,
/// with the route macros of those that have routes (`spore_routes!`; optional).
///
/// ```ignore
/// hypha_host_axum::deployment! { pub enum WithStripe {
///     spores: [hypha_spore_stripe::Stripe],
///     routes: [hypha_spore_stripe::stripe_routes],
/// } }
/// ```
///
/// `host!` declares one named `Deployment`; tests declare their own to build a host with
/// test spores.
#[macro_export]
macro_rules! deployment {
    ($vis:vis enum $name:ident {
        spores: [$($spore:ty),* $(,)?]
        $(, routes: [$($($routes:ident)::+),* $(,)?])? $(,)?
    }) => {
        /// The spores this host is built with (`hypha_host_axum::Deployment`).
        $vis enum $name {}

        impl $crate::Deployment for $name {
            type Spores = ($($spore,)*);

            fn router() -> $crate::__private::Router<
                $crate::__private::SharedState<$crate::PgStorage<Self>>,
            > {
                $crate::__private::hypha_http::router!($crate::PgStorage<$name>)
                    $($(.merge($($routes)::+!(router $crate::PgStorage<$name>)))*)?
                    // Again after the spores' routes: axum gives the fallback only to routes
                    // already there, and every error is `{code, error, details}`.
                    .method_not_allowed_fallback(|| async {
                        $crate::__private::hypha_http::error::ApiError::method_not_allowed()
                    })
            }

            fn spore_rows() -> ::std::vec::Vec<$crate::__private::hypha_http::routes::openapi::Row> {
                #[allow(unused_mut)]
                let mut rows = ::std::vec::Vec::new();
                $($(rows.extend($($routes)::+!(rows));)*)?
                rows
            }

            fn dispatch(
                state: $crate::__private::SharedState<$crate::PgStorage<Self>>,
            ) -> $crate::__private::BoxFuture<$crate::__private::Step> {
                ::std::boxed::Box::pin(async move {
                    $crate::__private::hypha_http::jobs::dispatch(&state).await
                })
            }
        }
    };
}

/// The native host's binary, built with `spores`: its whole `main.rs`.
///
/// ```ignore
/// hypha_host_axum::host! { spores: [hypha_spore_stripe::Stripe] }
/// ```
///
/// `spores: []` is the stock host. It declares a type named `Deployment` and `fn main`.
/// Spores with routes add their route macros: `routes: [hypha_spore_stripe::stripe_routes]`.
#[macro_export]
macro_rules! host {
    (spores: [$($spore:ty),* $(,)?] $(, routes: [$($($routes:ident)::+),* $(,)?])? $(,)?) => {
        $crate::deployment! { enum Deployment {
            spores: [$($spore),*],
            routes: [$($($($routes)::+),*)?],
        } }

        fn main() {
            $crate::run::<Deployment>();
        }
    };
}

// The stock deployment, for this crate's own tests (the binary declares its own).
#[cfg(test)]
deployment! { pub(crate) enum Stock { spores: [] } }
