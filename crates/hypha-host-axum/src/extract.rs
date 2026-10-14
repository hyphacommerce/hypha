//! Wrappers around axum's extractors whose rejections are `ApiError`s, so a
//! malformed body or path ID gets the same `{"error": ...}` JSON as everything else.

use axum::{
    Json,
    body::Body,
    extract::{FromRequest, FromRequestParts, Request},
    http::{HeaderValue, StatusCode, request::Parts},
    response::{IntoResponse, Response},
};
use hypha_core::{Ctx, IdempotencyRequest};
use serde::{Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};

use crate::error::ApiError;

#[derive(FromRequest)]
#[from_request(via(axum::Json), rejection(ApiError))]
pub struct JsonBody<T>(pub T);

#[derive(FromRequestParts)]
#[from_request(via(axum::extract::Path), rejection(ApiError))]
pub struct PathId<T>(pub T);

/// Who is calling: the `Ctx` every command checks its scopes against. This is the one
/// place a request turns into an actor with scopes, so real auth (API keys, sessions)
/// lands here and nowhere else.
pub struct Caller(pub Ctx);

impl<S: Send + Sync> FromRequestParts<S> for Caller {
    type Rejection = ApiError;

    async fn from_request_parts(_parts: &mut Parts, _state: &S) -> Result<Self, ApiError> {
        // TODO: real auth. Until then every request acts as the system, with every scope.
        Ok(Caller(Ctx::system()))
    }
}

/// The largest body `KeyedJson` reads: axum's default limit for `Json`.
const MAX_BODY: usize = 2 * 1024 * 1024;

/// A JSON body like `JsonBody`, plus the request's `Idempotency-Key` header (design
/// note 0007, section 5). The key goes to the command as sent, which checks it; the
/// fingerprint is SHA-256 over method, path and body, so the same key with a different
/// request is caught.
pub struct KeyedJson<T> {
    pub body: T,
    pub idempotency: Option<IdempotencyRequest>,
}

impl<S: Send + Sync, T: DeserializeOwned> FromRequest<S> for KeyedJson<T> {
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, ApiError> {
        let (parts, body) = req.into_parts();
        // Not checked here: a key that isn't visible ASCII reaches the command, which
        // refuses it with `invalid_idempotency_key` like any other bad key.
        let key = parts
            .headers
            .get("idempotency-key")
            .map(|value| String::from_utf8_lossy(value.as_bytes()).into_owned());
        let bytes = axum::body::to_bytes(body, MAX_BODY)
            .await
            .map_err(|_| ApiError::body_too_large(MAX_BODY))?;

        let idempotency = key.map(|key| {
            let target = parts.uri.path_and_query().map_or("", |p| p.as_str());
            let mut hash = Sha256::new();
            for part in [parts.method.as_str().as_bytes(), target.as_bytes(), &bytes] {
                hash.update(part);
                // A separator, so moving bytes between parts changes the fingerprint.
                hash.update([0]);
            }
            IdempotencyRequest {
                key,
                fingerprint: hash.finalize().into(),
            }
        });

        // `JsonBody` checks the content type and parses, with the usual rejections.
        let JsonBody(body) =
            JsonBody::from_request(Request::from_parts(parts, Body::from(bytes)), state).await?;
        Ok(Self { body, idempotency })
    }
}

/// `status` and `body`, with `Idempotent-Replayed: true` when the command replayed an
/// earlier request instead of running (Stripe's header name).
pub fn idempotent_response(status: StatusCode, body: impl Serialize, replayed: bool) -> Response {
    let mut response = (status, Json(body)).into_response();
    if replayed {
        response
            .headers_mut()
            .insert("idempotent-replayed", HeaderValue::from_static("true"));
    }
    response
}
