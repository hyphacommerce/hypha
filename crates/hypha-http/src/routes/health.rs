//! For load balancers and orchestrators: is this instance able to serve requests?

use axum::{Json, extract::State};
use serde_json::{Value, json};

use crate::{
    error::ApiError,
    state::{SharedState, Storage},
};

/// 200 `{"status":"ok"}` when storage hands out ports (on Postgres: the pool gives a
/// connection), otherwise the usual 500. Needs no auth and reads no data.
pub async fn check<S: Storage>(
    State(state): State<SharedState<S>>,
) -> Result<Json<Value>, ApiError> {
    state.public_ports().await?;
    Ok(Json(json!({ "status": "ok" })))
}
