//! Wrappers around axum's extractors whose rejections are `ApiError`s, so a
//! malformed body or path ID gets the same `{"error": ...}` JSON as everything else.

use axum::{
    Json,
    body::Body,
    extract::{FromRequest, FromRequestParts, Request},
    http::{HeaderValue, StatusCode, header::AUTHORIZATION, request::Parts},
    response::{IntoResponse, Response},
};
use hypha_auth::{HasApiKeys, TokenKind, authenticate_api_key};
use hypha_core::{Ctx, IdempotencyRequest};
use hypha_users::{StaffSession, authenticate_staff};
use serde::{Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};

use crate::{
    error::ApiError,
    state::{MemoryStorage, PgStorage, SharedState, Storage},
};

#[derive(FromRequest)]
#[from_request(via(axum::Json), rejection(ApiError))]
pub struct JsonBody<T>(pub T);

#[derive(FromRequestParts)]
#[from_request(via(axum::extract::Path), rejection(ApiError))]
pub struct PathId<T>(pub T);

/// Who is calling: the `Ctx` every command checks its scopes against. This is the one
/// place a request turns into an actor with scopes (design note 0008, section 3):
/// - `Authorization: Bearer hypha_sess_…`: a staff session, with the scopes of the
///   user's role, read fresh on every request;
/// - `Authorization: Bearer hypha_sk_…` or `hypha_pk_…`: an API key with its scopes;
/// - `X-Publishable-Key: hypha_pk_…`: a publishable key, for storefronts that will later
///   send a customer's session in `Authorization` beside it.
///
/// Nothing at all is `401 unauthenticated`; anything unknown, expired or revoked is
/// `401 invalid_token`.
pub struct Caller(pub Ctx);

/// A staff member's session, for the routes about "me" (password, 2FA, logout). An API
/// key gets `401 session_required`.
pub struct StaffCaller(pub StaffSession);

/// What a request authenticated as.
enum Authenticated {
    Staff(Box<StaffSession>),
    Key(Ctx),
}

/// The token a request carries, if any, and whether it came as a publishable key.
fn credential(parts: &Parts) -> Result<Option<(String, bool)>, ApiError> {
    let invalid = || ApiError::from(hypha_auth::CommandError::InvalidToken);
    if let Some(value) = parts.headers.get(AUTHORIZATION) {
        let value = value.to_str().map_err(|_| invalid())?;
        let (scheme, token) = value.split_once(' ').ok_or_else(invalid)?;
        if !scheme.eq_ignore_ascii_case("bearer") {
            return Err(invalid());
        }
        return Ok(Some((token.trim().to_string(), false)));
    }
    if let Some(value) = parts.headers.get("x-publishable-key") {
        let token = value.to_str().map_err(|_| invalid())?.trim().to_string();
        return Ok(Some((token, true)));
    }
    Ok(None)
}

async fn authenticate<S: Storage>(
    parts: &Parts,
    state: &SharedState<S>,
) -> Result<Authenticated, ApiError> {
    let Some((token, publishable_header)) = credential(parts)? else {
        return Err(ApiError::unauthenticated());
    };
    let ports = state.ports().await?;
    match TokenKind::of(&token) {
        Some(TokenKind::Session) if !publishable_header => {
            let staff = authenticate_staff(&ports, &state.clock, &token).await?;
            Ok(Authenticated::Staff(Box::new(staff)))
        }
        Some(TokenKind::SecretKey) if !publishable_header => Ok(Authenticated::Key(
            authenticate_api_key(ports.api_keys(), &token).await?,
        )),
        Some(TokenKind::PublishableKey) => Ok(Authenticated::Key(
            authenticate_api_key(ports.api_keys(), &token).await?,
        )),
        _ => Err(hypha_auth::CommandError::InvalidToken.into()),
    }
}

/// The extractors, once per concrete storage. axum needs an extractor's future to be
/// `Send`, and with our `Send`-free repo traits the compiler can only prove that for a
/// concrete type (the same reason as `routes::router!`).
macro_rules! extractors {
    ($($storage:ty),*) => {$(
        impl FromRequestParts<SharedState<$storage>> for Caller {
            type Rejection = ApiError;

            async fn from_request_parts(
                parts: &mut Parts,
                state: &SharedState<$storage>,
            ) -> Result<Self, ApiError> {
                Ok(match authenticate(parts, state).await? {
                    Authenticated::Staff(staff) => Caller(staff.ctx),
                    Authenticated::Key(ctx) => Caller(ctx),
                })
            }
        }

        impl FromRequestParts<SharedState<$storage>> for StaffCaller {
            type Rejection = ApiError;

            async fn from_request_parts(
                parts: &mut Parts,
                state: &SharedState<$storage>,
            ) -> Result<Self, ApiError> {
                match authenticate(parts, state).await? {
                    Authenticated::Staff(staff) => Ok(StaffCaller(*staff)),
                    Authenticated::Key(_) => Err(ApiError::session_required()),
                }
            }
        }
    )*};
}

extractors!(MemoryStorage, PgStorage);

/// The largest body `KeyedJson` reads: axum's default limit for `Json`.
const MAX_BODY: usize = 2 * 1024 * 1024;

/// A JSON body like `JsonBody`, plus the request's `Idempotency-Key` header (design
/// note 0007, section 5). The key goes to the command as sent, which checks it; the
/// fingerprint is SHA-256 over the method, the path (lowercased: IDs are
/// case-insensitive) with its query string, and the body in canonical JSON, so the same
/// key with a different request is caught but a retry that re-serialized its body
/// (other whitespace or key order) is not.
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
            let path = parts.uri.path().to_ascii_lowercase();
            let query = parts.uri.query().unwrap_or("");
            let body = canonical_json(&bytes);
            let mut hash = Sha256::new();
            for part in [
                parts.method.as_str().as_bytes(),
                path.as_bytes(),
                query.as_bytes(),
                &body,
            ] {
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

/// `body` as canonical JSON: no whitespace, object keys sorted, at every depth. A body
/// that isn't JSON is used as sent (parsing refuses it right after anyway).
fn canonical_json(body: &[u8]) -> Vec<u8> {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) else {
        return body.to_vec();
    };
    let mut out = Vec::new();
    write_canonical(&value, &mut out);
    out
}

fn write_canonical(value: &serde_json::Value, out: &mut Vec<u8>) {
    use serde_json::Value;
    match value {
        // Sorted here rather than trusting `Map`'s order: with serde_json's
        // `preserve_order` feature (which any crate in the build can turn on), a map keeps
        // the order it was sent in.
        Value::Object(map) => {
            let mut entries: Vec<_> = map.iter().collect();
            entries.sort_by(|a, b| a.0.cmp(b.0));
            out.push(b'{');
            for (n, (key, value)) in entries.into_iter().enumerate() {
                if n > 0 {
                    out.push(b',');
                }
                write_canonical(&Value::String(key.clone()), out);
                out.push(b':');
                write_canonical(value, out);
            }
            out.push(b'}');
        }
        Value::Array(items) => {
            out.push(b'[');
            for (n, item) in items.iter().enumerate() {
                if n > 0 {
                    out.push(b',');
                }
                write_canonical(item, out);
            }
            out.push(b']');
        }
        // Scalars print one way already (strings escaped, numbers as parsed).
        scalar => out.extend_from_slice(scalar.to_string().as_bytes()),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_json_ignores_whitespace_and_key_order() {
        let a = canonical_json(br#"{"b": [1, {"y": 2, "x": "\u00e9"}], "a": null}"#);
        let b = canonical_json(br#"{"a":null,"b":[1,{"x":"\u00e9","y":2}]}"#);
        assert_eq!(a, b);
        assert_eq!(a, r#"{"a":null,"b":[1,{"x":"é","y":2}]}"#.as_bytes());
    }

    #[test]
    fn canonical_json_keeps_array_order_and_values() {
        assert_ne!(canonical_json(b"[1,2]"), canonical_json(b"[2,1]"));
        assert_ne!(
            canonical_json(br#"{"a":1}"#),
            canonical_json(br#"{"a":"1"}"#)
        );
    }

    #[test]
    fn a_body_that_is_not_json_is_used_as_sent() {
        assert_eq!(canonical_json(b"not json"), b"not json".to_vec());
    }
}
