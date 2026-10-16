//! Wrappers around axum's extractors whose rejections are `ApiError`s, so a
//! malformed body or path ID gets the same `{"error": ...}` JSON as everything else.
//!
//! Order matters for security: a handler first turns its `Caller` into ports and a
//! `Ctx` (`Caller::authenticate`), and only then parses its body (`JsonBody::parse`,
//! `KeyedJson::parse`). So a request with a bad token gets its 401 before any JSON is
//! parsed or fingerprinted, and learns nothing about what a valid body looks like.

use std::marker::PhantomData;

use axum::{
    Json,
    body::Bytes,
    extract::{FromRequest, FromRequestParts, Request},
    http::{
        HeaderValue, StatusCode,
        header::{AUTHORIZATION, CONTENT_TYPE},
        request::Parts,
    },
    response::{IntoResponse, Response},
};
use hypha_auth::{HasApiKeys, TokenKind, authenticate_api_key};
use hypha_core::ActingAs;
use hypha_core::{Clock, Ctx, IdempotencyRequest};
use hypha_users::{StaffPorts, StaffSession, authenticate_staff};
use serde::{Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};

use crate::{
    error::ApiError,
    state::{AppState, SharedState, Storage},
};

/// A JSON body, read but not yet parsed: call `parse` after authenticating (see the
/// module docs). Checks the content type and parses with axum's rules, mapping its
/// rejections to `ApiError`s like every other error.
pub struct JsonBody<T> {
    content_type: Option<HeaderValue>,
    bytes: Bytes,
    _body: PhantomData<fn() -> T>,
}

/// Reads at most `server.max_body` bytes (2 MiB by default, axum's limit for `Json`).
impl<S: Storage, T> FromRequest<SharedState<S>> for JsonBody<T> {
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &SharedState<S>) -> Result<Self, ApiError> {
        let limit = state.config.server.max_body;
        let (parts, body) = req.into_parts();
        let bytes = axum::body::to_bytes(body, limit)
            .await
            .map_err(|_| ApiError::body_too_large(limit))?;
        Ok(Self {
            content_type: parts.headers.get(CONTENT_TYPE).cloned(),
            bytes,
            _body: PhantomData,
        })
    }
}

impl<T: DeserializeOwned> JsonBody<T> {
    pub fn parse(self) -> Result<T, ApiError> {
        if !is_json(self.content_type.as_ref()) {
            return Err(ApiError::missing_content_type());
        }
        let Json(body) = Json::<T>::from_bytes(&self.bytes)?;
        Ok(body)
    }
}

/// `application/json` or `application/<something>+json`, parameters ignored, as axum's
/// `Json` accepts.
fn is_json(content_type: Option<&HeaderValue>) -> bool {
    let Some(mime) = content_type.and_then(|v| v.to_str().ok()) else {
        return false;
    };
    let mime = mime
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    mime == "application/json" || (mime.starts_with("application/") && mime.ends_with("+json"))
}

#[derive(FromRequestParts)]
#[from_request(via(axum::extract::Path), rejection(ApiError))]
pub struct PathId<T>(pub T);

/// Who is calling: the credentials a request carries (design note 0008, section 3).
/// The extractor only reads headers; the handler's first step is `authenticate`, which
/// opens the request's ports (one pooled connection per request) and resolves the
/// `Ctx` every command checks its scopes against. Ports come only from there (or from
/// `AppState::public_ports`, named for the few routes that need no credentials), so a
/// handler can't reach storage without authenticating.
///
/// - `Authorization: Bearer hypha_sess_…`: a staff session, with the scopes of the
///   user's role, read fresh on every request;
/// - `Authorization: Bearer hypha_sk_…` or `hypha_pk_…`: an API key with its scopes;
/// - `X-Publishable-Key: hypha_pk_…`: a publishable key, for storefronts that will later
///   send a customer's session in `Authorization` beside it.
///
/// Nothing at all is `401 unauthenticated` (from the extractor); anything unknown,
/// expired or revoked is `401 invalid_token` (from `authenticate`).
///
/// Each surface takes its own kind of credential (design note 0010, S3): `Caller` (that
/// is, `Caller<Admin>`) a staff session or a secret key, `Caller<Storefront>` a
/// publishable key. The wrong kind is a 403 before anything is looked up, so a
/// publishable key never reaches an admin command, whatever its scopes.
pub struct Caller<A: Surface = Admin>(Credential, PhantomData<A>);

/// A staff member's session, for the routes about "me" (password, 2FA, logout), on the
/// admin surface. An API key gets `401 session_required`.
pub struct StaffCaller(Credential);

/// Which credentials an API surface takes.
pub trait Surface {
    /// Refuses a credential of the wrong kind for this surface: `kind` is what the
    /// token's prefix says (`None` if it isn't one of ours), `publishable_header`
    /// whether it came in `X-Publishable-Key`.
    fn admit(kind: Option<TokenKind>, publishable_header: bool) -> Result<(), ApiError>;
}

/// `/admin/*`: staff sessions and secret keys.
pub struct Admin;

/// `/storefront/*`: publishable keys (customer sessions later).
pub struct Storefront;

impl Surface for Admin {
    fn admit(kind: Option<TokenKind>, publishable_header: bool) -> Result<(), ApiError> {
        match kind {
            Some(TokenKind::PublishableKey) => Err(ApiError::admin_credentials_required()),
            Some(TokenKind::Session | TokenKind::SecretKey) if !publishable_header => Ok(()),
            _ => Err(hypha_auth::CommandError::InvalidToken.into()),
        }
    }
}

impl Surface for Storefront {
    fn admit(kind: Option<TokenKind>, _publishable_header: bool) -> Result<(), ApiError> {
        match kind {
            Some(TokenKind::PublishableKey) => Ok(()),
            Some(TokenKind::Session | TokenKind::SecretKey) => {
                Err(ApiError::publishable_key_required())
            }
            _ => Err(hypha_auth::CommandError::InvalidToken.into()),
        }
    }
}

struct Credential {
    token: String,
    /// Sent as `X-Publishable-Key`, where only publishable keys are accepted.
    publishable_header: bool,
}

/// What a request authenticated as.
enum Authenticated {
    Staff(Box<StaffSession>),
    Key(Ctx),
}

/// The token a request carries; `unauthenticated` if none.
fn credential(parts: &Parts) -> Result<Credential, ApiError> {
    let invalid = || ApiError::from(hypha_auth::CommandError::InvalidToken);
    if let Some(value) = parts.headers.get(AUTHORIZATION) {
        let value = value.to_str().map_err(|_| invalid())?;
        let (scheme, token) = value.split_once(' ').ok_or_else(invalid)?;
        if !scheme.eq_ignore_ascii_case("bearer") {
            return Err(invalid());
        }
        return Ok(Credential {
            token: token.trim().to_string(),
            publishable_header: false,
        });
    }
    if let Some(value) = parts.headers.get("x-publishable-key") {
        let token = value.to_str().map_err(|_| invalid())?.trim().to_string();
        return Ok(Credential {
            token,
            publishable_header: true,
        });
    }
    Err(ApiError::unauthenticated())
}

impl Credential {
    async fn authenticate<P: StaffPorts + HasApiKeys>(
        &self,
        ports: &P,
        clock: &impl Clock,
    ) -> Result<Authenticated, ApiError> {
        let token = self.token.as_str();
        match TokenKind::of(token) {
            Some(TokenKind::Session) if !self.publishable_header => {
                let staff = authenticate_staff(ports, clock, token).await?;
                Ok(Authenticated::Staff(Box::new(staff)))
            }
            Some(TokenKind::SecretKey) if !self.publishable_header => Ok(Authenticated::Key(
                authenticate_api_key(ports.api_keys(), token).await?,
            )),
            Some(TokenKind::PublishableKey) => Ok(Authenticated::Key(
                authenticate_api_key(ports.api_keys(), token).await?,
            )),
            _ => Err(hypha_auth::CommandError::InvalidToken.into()),
        }
    }
}

impl<A: Surface> Caller<A> {
    /// The request's ports and its `Ctx` (a staff member's role scopes or a key's). The
    /// only way an authenticated handler gets ports: storage is reachable through the
    /// credentials, so no handler can use it without them.
    pub async fn authenticate<S: Storage>(
        self,
        state: &AppState<S>,
    ) -> Result<(S::Ports<'_>, Ctx), ApiError> {
        A::admit(TokenKind::of(&self.0.token), self.0.publishable_header)?;
        let ports = state.public_ports().await?;
        let ctx = match self.0.authenticate(&ports, &state.clock).await? {
            Authenticated::Staff(staff) => staff.ctx,
            Authenticated::Key(ctx) => ctx,
        };
        // Every event these ports write from here on is the caller's (design note 0016).
        ports.act_as(ctx.actor());
        Ok((ports, ctx))
    }
}

impl StaffCaller {
    /// The request's ports and the staff session behind these credentials; an API key
    /// is `session_required`.
    pub async fn authenticate<S: Storage>(
        self,
        state: &AppState<S>,
    ) -> Result<(S::Ports<'_>, StaffSession), ApiError> {
        Admin::admit(TokenKind::of(&self.0.token), self.0.publishable_header)?;
        let ports = state.public_ports().await?;
        match self.0.authenticate(&ports, &state.clock).await? {
            Authenticated::Staff(staff) => {
                ports.act_as(staff.ctx.actor());
                Ok((ports, *staff))
            }
            Authenticated::Key(_) => Err(ApiError::session_required()),
        }
    }
}

impl<S: Send + Sync, A: Surface> FromRequestParts<S> for Caller<A> {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, ApiError> {
        credential(parts).map(|c| Caller(c, PhantomData))
    }
}

impl<S: Send + Sync> FromRequestParts<S> for StaffCaller {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, ApiError> {
        credential(parts).map(StaffCaller)
    }
}

/// A JSON body like `JsonBody`, plus the request's `Idempotency-Key` header (design
/// note 0007, section 5); `parse` (after authenticating) returns both. The key goes to
/// the command as sent, which checks it; the fingerprint is SHA-256 over the method, the
/// path (lowercased: IDs are case-insensitive) with its query string, and the body in
/// canonical JSON, so the same key with a different request is caught but a retry that
/// re-serialized its body (other whitespace or key order) is not.
pub struct KeyedJson<T> {
    body: JsonBody<T>,
    key: Option<String>,
    method: String,
    path: String,
    query: String,
}

/// A parsed `KeyedJson`.
pub struct Keyed<T> {
    pub body: T,
    pub idempotency: Option<IdempotencyRequest>,
}

impl<S: Storage, T> FromRequest<SharedState<S>> for KeyedJson<T> {
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &SharedState<S>) -> Result<Self, ApiError> {
        // Not checked here: a key that isn't visible ASCII reaches the command, which
        // refuses it with `invalid_idempotency_key` like any other bad key.
        let key = req
            .headers()
            .get("idempotency-key")
            .map(|value| String::from_utf8_lossy(value.as_bytes()).into_owned());
        let method = req.method().as_str().to_string();
        let path = req.uri().path().to_ascii_lowercase();
        let query = req.uri().query().unwrap_or("").to_string();
        let body = JsonBody::from_request(req, state).await?;
        Ok(Self {
            body,
            key,
            method,
            path,
            query,
        })
    }
}

impl<T: DeserializeOwned> KeyedJson<T> {
    pub fn parse(self) -> Result<Keyed<T>, ApiError> {
        let idempotency = self.key.map(|key| {
            let body = canonical_json(&self.body.bytes);
            let mut hash = Sha256::new();
            for part in [
                self.method.as_bytes(),
                self.path.as_bytes(),
                self.query.as_bytes(),
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
        Ok(Keyed {
            body: self.body.parse()?,
            idempotency,
        })
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
