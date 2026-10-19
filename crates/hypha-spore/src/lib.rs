//! Everything a spore may use (design note 0030, §4, SP6): a spore depends on this crate,
//! not on Hypha's internal ones, so an internal refactor breaks spores only when this
//! crate changes. It is public API: each item here is a promise, versioned on its own
//! (0.x, breaking changes on minor versions until 1.0).
//!
//! - the crate root: the [`Spore`] trait, its environment ([`SporeEnv`]) and errors,
//!   and what it contributes to frontends ([`ui`]) and to the settings form
//!   ([`settings`]);
//! - [`core`]: Hypha's shared types (IDs, money, `Ctx` and scopes, time, errors) and
//!   generic ports (clock, IDs, randomness, the `SecretBox`, events);
//! - [`client`]: the host's HTTP client, for calling other services;
//! - [`migrations`]: a spore's own tables (`MigrationSet`);
//! - [`payments`], [`orders`], [`order_actions`]: the domain ports a spore implements
//!   and the commands and read repositories it calls, by domain. Not the `Stage*`
//!   traits of core aggregates: a spore changes core data through commands (SP4);
//! - [`postgres`] (feature `postgres`): what a spore's repositories over its own tables
//!   are written with;
//! - [`http`] (feature `http`): what a spore's routes are written with, and
//!   `spore_routes!`;
//! - [`testing`] (feature `testing`): in-memory ports and deterministic clocks and IDs.
//!
//! Reaching past this crate into an internal one works (they're published) but comes
//! with no stability promise.

pub use hypha_spore_core::{Spore, SporeEnv, SporeError};

/// Building a spore: its environment and what it can fail with.
pub mod env {
    pub use hypha_spore_core::{SettingsError, SporeEnv, SporeError};
}

/// The host's HTTP client (`SporeEnv` is one): requests with a body and headers, the
/// answer capped, no redirects followed, the operator's address policy applied
/// (`outbound.allow_private`).
pub mod client {
    pub use hypha_spore_core::{HttpClient, HttpError, HttpMethod, HttpRequest, HttpResponse};
}

/// A spore's settings (design note 0030, §8): the problems its `check_settings`
/// reports, the schema subset its form is written in, and the port it reads them
/// through (`SporeEnv::settings` does the reading).
pub mod settings {
    pub use hypha_apps::{HasSporeSettings, SECRET_MARKER, SettingsProblem, SettingsSchema};
}

/// Hypha's shared types and generic ports: no domain logic.
pub mod core {
    pub use hypha_core::{
        Actor, AppId, AppKey, AuthError, CartId, Clock, Ctx, Currency, CustomerId, ErrorCode,
        EventHandler, EventId, FulfillmentId, IdGenerator, Money, MoneyError, OrderId, OrderLineId,
        PaymentId, PaymentMethodId, ProductId, Random, RepoError, Scope, SealError, Sealed,
        SecretBox, StoredEvent, Timestamp, VariantId,
    };
}

/// A spore's own tables (SP4): module `spore_<name>`, tables `spore_<name>_*`, checked
/// when they're applied (§5).
pub mod migrations {
    pub use hypha_store_postgres::{Migration, MigrationSet};
}

/// Payments (design note 0027): the provider port a spore implements, and reading
/// payments and methods.
pub mod payments {
    pub use hypha_payments::{
        ActionAnswer, ActionRequest, HasPaymentMethods, HasPayments, NextAction, Payment,
        PaymentAction, PaymentError, PaymentMethod, PaymentMethodRepo, PaymentProvider,
        PaymentProviders, PaymentRepo, PaymentRequest, PaymentStatus, ProviderKey, Started,
        get_payment, no_settings,
    };
}

/// Orders: reading them.
pub mod orders {
    pub use hypha_orders::{
        FinancialStatus, FulfillmentStatus, HasOrders, Order, OrderLine, OrderRepo, get_order,
    };
}

/// The order workflows a spore calls: reporting what happened to a payment
/// (`record_payment_event`, idempotent by its key), and the ports they need.
pub mod order_actions {
    pub use hypha_order_actions::{
        CommandError, OrderActionPorts, PaymentEvent, PaymentEventKind, record_payment_event,
    };
}

/// The storage kit (feature `postgres`): a spore's repositories over its own tables
/// implement its accessor traits for `PgStore` and query through `conn().await.db()`.
/// `spore_repo_error` maps its unique constraints to `RepoError::Duplicate`.
#[cfg(feature = "postgres")]
pub mod postgres {
    pub use hypha_store_postgres::{Db, PgConnection, PgStore, Queries, spore_repo_error};
}

/// The HTTP kit (feature `http`): a spore's handlers take `State<SharedState<S>>` and a
/// `Caller` like core's, and call `caller.authenticate(&state)` for their ports and
/// `Ctx`.
#[cfg(feature = "http")]
pub mod http {
    /// For `#[utoipa::path]` and `#[derive(utoipa::ToSchema)]` on a spore's handlers and
    /// DTOs (`use hypha_spore::http::utoipa;`), which the host's `openapi` command reads.
    pub use hypha_http::__private::utoipa;
    pub use hypha_http::error::ApiError;
    pub use hypha_http::extract::{Admin, Caller, JsonBody, PathId, Query, Storefront};
    pub use hypha_http::spore_routes;
    pub use hypha_http::spores::{SporeSurface, spore};
    pub use hypha_http::state::{AppState, SharedState, Storage};
}

/// The test kit (feature `testing`): in-memory ports for a spore's command tests, a
/// deterministic clock and IDs, and a host for building it (`host()`).
#[cfg(feature = "testing")]
pub mod testing {
    pub use hypha_spore_core::env::testing::host;
    pub use hypha_testkit::{FixedClock, InMemoryPorts, SequentialIds, TestEnv};
}
