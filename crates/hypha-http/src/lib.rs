//! Hypha's HTTP layer (design note 0026, W3): the routes, their extractors, error
//! mapping and DTOs, the OpenAPI documents and the host state, as a library the hosts
//! build on. `hypha-host-axum` is the binary that serves it; a Workers host will be the
//! second (0026).
//!
//! Moved here unchanged from `hypha-host-axum` (a pure move): it still holds the
//! Postgres pool, the job runner and the other Tokio-only pieces, which move back into
//! the native host as they get traits the Workers host can implement too (0026, §6).

pub mod client_ip;
pub mod error;
pub mod extract;
pub mod handlers;
pub mod jobs;
pub mod key_cache;
pub mod media;
pub mod outbound;
pub mod patch;
pub mod rate_limit;
pub mod routes;
pub mod shipping;
pub mod state;
pub mod webhooks;
