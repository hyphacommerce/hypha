//! Wrappers around axum's extractors whose rejections are `ApiError`s, so a
//! malformed body or path ID gets the same `{"error": ...}` JSON as everything else.

use axum::extract::{FromRequest, FromRequestParts};

use crate::error::ApiError;

#[derive(FromRequest)]
#[from_request(via(axum::Json), rejection(ApiError))]
pub struct JsonBody<T>(pub T);

#[derive(FromRequestParts)]
#[from_request(via(axum::extract::Path), rejection(ApiError))]
pub struct PathId<T>(pub T);
