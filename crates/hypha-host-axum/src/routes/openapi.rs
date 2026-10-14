//! The OpenAPI documents (design note 0010, S6). Filled in by the next change.

use axum::{Json, extract::State};
use serde_json::{Value, json};

use crate::state::SharedState;

/// `GET /admin/openapi.json`.
pub(super) async fn admin_document<S>(State(_): State<SharedState<S>>) -> Json<Value> {
    Json(json!({}))
}

/// `GET /storefront/openapi.json`.
pub(super) async fn storefront_document<S>(State(_): State<SharedState<S>>) -> Json<Value> {
    Json(json!({}))
}
