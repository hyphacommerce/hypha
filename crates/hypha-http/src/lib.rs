//! Hypha's HTTP layer (design note 0026, W3): the routes, their extractors, error
//! mapping and DTOs, the OpenAPI documents and the host state, as a library the hosts
//! build on. `hypha-host-axum` is the binary that serves it; a Workers host will be the
//! second (0026).
//!
//! What only a runtime can do (time, IDs, randomness, CPU-heavy work, background work,
//! rate limits, outbound HTTP) comes from the host (`host.rs`); the storage too
//! (`state::Storage`). So this crate holds no runtime: the Postgres pool, the job runner
//! and the Tokio pieces are `hypha-host-axum`'s (design note 0026, §1).

pub mod client_ip;
pub mod email;
pub mod error;
pub mod extract;
pub mod host;
pub mod jobs;
pub mod key_cache;
pub mod mail;
pub mod media;
pub mod outbound;
pub mod patch;
pub mod rate_limit;
pub mod routes;
pub mod shipping;
pub mod state;
pub mod webhooks;
