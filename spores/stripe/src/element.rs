//! `<stripe-payment>`, the storefront's payment widget (`docs/apps/elements.md`): the
//! Stripe app's element as is (`element/payment-element.ts`, bundled into
//! `element/payment-element.js`), served from a public route so any storefront can load it
//! as an ES module from the API's address. It loads Stripe.js from js.stripe.com, mounts
//! the Payment Element, confirms with `redirect: "if_required"` and emits
//! `hypha:payment-confirmed {paymentId}` or `hypha:error {message}`.

use hypha_spore::http::axum::http::{HeaderValue, header};
use hypha_spore::http::axum::response::{IntoResponse, Response};
use hypha_spore::http::{Storage, utoipa};

/// Where it's served (`UiElement::path`).
pub const PATH: &str = "/spores/stripe/elements/payment.js";

/// The bundle, built from `element/payment-element.ts`.
pub const MODULE: &str = include_str!("../element/payment-element.js");

/// The element's ES module. Generic over the storage like every spore route (the host
/// names its storage when it expands the routes), though it reads none.
#[utoipa::path(
    get,
    path = "/spores/stripe/elements/payment.js",
    responses((status = 200, description = "The `<stripe-payment>` element's ES module", content_type = "text/javascript", body = String)),
    security(()),
)]
pub async fn payment_js<S: Storage>() -> Response {
    let mut response = MODULE.into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/javascript; charset=utf-8"),
    );
    // A module loads cross-origin, from the storefront's page.
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=300"),
    );
    response
}
