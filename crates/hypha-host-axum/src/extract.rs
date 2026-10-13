//! Wrappers around axum's extractors whose rejections are `ApiError`s, so a
//! malformed body or path ID gets the same `{"error": ...}` JSON as everything else.

use axum::{
    extract::{FromRequest, FromRequestParts},
    http::request::Parts,
};
use hypha_core::Ctx;

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
