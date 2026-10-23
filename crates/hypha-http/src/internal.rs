//! Sending a request through the host's own router, in process: how an agent's call
//! reaches an operation (`agents.rs`, and `/mcp` on it) and how an approved proposal is
//! applied. The request goes through the same middleware, extractors, handlers, errors,
//! redaction, idempotency and previews as one from outside; nothing is implemented twice.
//!
//! The host installs its router once it's built (`AppState::install`), without its state,
//! so the state holds no reference to itself; each call gives it the state again.
//!
//! An in-process request may carry an authenticated agent as a request extension
//! (`AgentRequest`): extensions never come from the wire, so only code in this crate can
//! make a request act as an agent. `Caller` builds the agent's `Ctx` from it.

use std::sync::{Arc, OnceLock};

use axum::{Router, body::Body, extract::Request, response::Response};
use hypha_core::{AgentId, Ctx};
use tower::ServiceExt;

use crate::error::ApiError;
use crate::operations::Operations;
use crate::state::{AppState, SharedState};

/// What the host installs once its router is built (`AppState::install`).
pub struct Internal<S> {
    router: OnceLock<Router<SharedState<S>>>,
    operations: OnceLock<Operations>,
}

impl<S> Default for Internal<S> {
    fn default() -> Self {
        Self {
            router: OnceLock::new(),
            operations: OnceLock::new(),
        }
    }
}

/// An agent a request acts as, authenticated and checked (`agents::call_as`): its `Ctx`
/// (`Actor::Agent`, the scopes its permissions come to now). Only this crate makes one,
/// and only in-process requests carry it.
#[derive(Clone)]
pub struct AgentRequest {
    pub(crate) agent: AgentId,
    pub(crate) ctx: Ctx,
}

impl AgentRequest {
    pub fn agent(&self) -> AgentId {
        self.agent
    }
}

impl<S: crate::state::Storage> AppState<S> {
    /// Installs the host's router (`router!` with its spores' routes and its layers, as
    /// served, without its state) and the operations its spores add (their `openapi`
    /// rows): what in-process requests and agents' operations go through. Once, at
    /// startup; later calls change nothing.
    pub fn install(
        &self,
        router: Router<SharedState<S>>,
        spore_rows: Vec<crate::routes::openapi::Row>,
    ) {
        let _ = self.internal.router.set(router);
        let _ = self.internal.operations.set(Operations::build(spore_rows));
    }

    /// The admin operations of this build: the route table's and the installed spores'
    /// (the table's alone before `install`).
    pub fn operations(&self) -> &Operations {
        self.internal
            .operations
            .get_or_init(|| Operations::build(Vec::new()))
    }
}

/// Sends `request` through the installed router, as `agent` when given, and answers what
/// the route answered. `500` when no router was installed (a host's bug).
pub async fn dispatch<S: crate::state::Storage>(
    state: &SharedState<S>,
    mut request: Request<Body>,
    agent: Option<AgentRequest>,
) -> Response {
    let Some(router) = state.internal.router.get() else {
        return axum::response::IntoResponse::into_response(ApiError::internal(
            "no router installed for in-process requests",
        ));
    };
    if let Some(agent) = agent {
        request.extensions_mut().insert(agent);
    }
    let service = router.clone().with_state(Arc::clone(state));
    match crate::preview::detached(service.oneshot(request)).await {
        Ok(response) => response,
        Err(never) => match never {},
    }
}
