//! Hypha's HTTP layer: the routes, their extractors, error mapping and DTOs, the OpenAPI
//! documents and the host state, as a library the hosts build on. `hypha-host-axum` is the
//! binary that serves it; a Workers host will be the second.
//!
//! What only a runtime can do (time, IDs, randomness, CPU-heavy work, background work, rate
//! limits, outbound HTTP) comes from the host (`host.rs`); the storage too
//! (`state::Storage`). So this crate holds no runtime: the Postgres pool, the job runner
//! and the Tokio pieces are `hypha-host-axum`'s.

pub mod agents;
pub mod app_endpoints;
pub mod app_fields;
pub mod client_ip;
pub mod email;
pub mod error;
pub mod extensions;
pub mod extract;
pub mod handlers;
pub mod host;
pub mod internal;
pub mod jobs;
pub mod key_cache;
pub mod mail;
pub mod manifests;
pub mod media;
pub mod operations;
pub mod outbound;
pub mod patch;
pub mod payments;
pub mod preview;
pub mod rate_limit;
pub mod redact;
pub mod routes;
pub mod search_app;
pub mod session_token;
pub mod shipping;
pub mod spores;
pub mod state;
pub mod webhooks;

/// What the macros' expansions (`spore_routes!`) name, so a spore or host crate needs no
/// dependency of its own on them.
#[doc(hidden)]
pub mod __private {
    pub use axum;
    pub use axum::Router;
    pub use axum::extract::{Request, State};
    pub use hypha_spore_core::Spore;
    pub use pastey;
    pub use utoipa;
}
