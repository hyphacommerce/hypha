//! After each successful request that may have written (anything but GET, HEAD and
//! OPTIONS), the host's `AfterWrites` runs: natively it wakes the dispatcher to deliver
//! the events the request recorded (design note 0025, §6); on Workers it runs a dispatch
//! step in `waitUntil` (0026, W6).

use axum::{
    extract::{Request, State},
    http::Method,
    middleware::Next,
    response::Response,
};

use crate::host::AfterWrites;

/// A layer (`axum::middleware::from_fn_with_state`) calling `hook` after writes.
pub async fn after_writes(
    State(hook): State<AfterWrites>,
    request: Request,
    next: Next,
) -> Response {
    let writes = !matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    );
    let response = next.run(request).await;
    if writes && response.status().is_success() {
        hook.notify();
    }
    response
}
