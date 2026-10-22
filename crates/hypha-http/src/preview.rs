//! Previewing an admin write, and applying exactly what was previewed.
//!
//! `Hypha-Preview: 1` on any non-GET request under `/admin/` previews it. The request runs
//! for real (the same handler, command and ports) and answers what it would have done
//! instead of doing it:
//!
//! - the ports it gets (`AppState::public_ports`, so `Caller::authenticate` too) run in a
//!   preview (`Storage::begin_preview`): one transaction, rolled back once the handler is
//!   done, after the events its writes recorded are read back;
//! - the host's capabilities (`host.rs`, the one door to effects outside the database)
//!   look for the request's `Preview` while they're called: emails are recorded, not sent;
//!   calls to apps are blocked unless the extension point is read-only; media storage is
//!   blocked; background work and wake-ups are dropped; the IDs handed out are noted.
//!
//! A preview that ran to its end answers a token (`hypha_pv_…`, stored as its SHA-256 for
//! `agents.preview_token_lifetime`), bound to the caller and the request (its
//! fingerprint: method, path, query and body), holding the IDs it was handed and the
//! digest of its events. Sending the same write with `Hypha-Preview-Token` applies it:
//!
//! - its ports run in one transaction too (`Storage::begin_apply`); `Caller::authenticate`
//!   checks the token against the caller and marks it used, in that transaction;
//! - the command is handed the preview's IDs first (`HostIds`), so what it creates is what
//!   the preview showed;
//! - once the handler is done, the events its writes recorded are digested like the
//!   preview's: equal, everything commits; different (something changed meanwhile), it's
//!   rolled back and answers `409 preview_stale` with what it would do now, and a token
//!   for that.
//!
//! The request's `Preview` (or `Apply`) is current while its future is polled
//! (`Scoped`), so a capability called from anywhere in the handler finds it without being
//! passed it: a poll runs one request's code at a time on its thread, natively and on
//! Workers alike.
//!
//! A write that fails answers its failure as it would have without the header; one that
//! reached a blocked effect answers `200` with `complete: false` and what blocked it, and
//! no token.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard};
use std::task::{Context, Poll};
use std::time::Duration;

use axum::{
    body::{Body, Bytes},
    extract::Request,
    http::{Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use hypha_auth::{NewToken, TokenHash, TokenKind};
use hypha_core::{
    Actor, Clock, Ctx, HasPreviewTokens, PreviewToken, PreviewTokenRepo, RepoError, StoredEvent,
    Timestamp,
};
use hypha_custom_fields::{CallerKind, CustomFieldDefinition, FieldCaller};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::error::ApiError;
use crate::host::{BoxFuture, HostClock, HostRandom};
use crate::outbound::CallPurpose;
use crate::redact::Redaction;

/// The request header that asks for a preview: `1`. `0` (or none) runs the write.
pub const PREVIEW_HEADER: &str = "hypha-preview";

/// The request header that applies what a preview showed: the preview's `token`.
pub const PREVIEW_TOKEN_HEADER: &str = "hypha-preview-token";

/// The most of a handler's answer a preview reads (its `response.body`).
const MAX_RESPONSE_BYTES: usize = 16 << 20;

/// The most of a request's body a preview or an apply reads to fingerprint it: the
/// largest `server.max_body` allows. The route still refuses what's over its own limit.
const MAX_REQUEST_BYTES: usize = 64 << 20;

thread_local! {
    /// The preview whose request is being polled (`Scoped`).
    static CURRENT: RefCell<Option<Arc<Preview>>> = const { RefCell::new(None) };
    /// The apply whose request is being polled (`Scoped`).
    static APPLYING: RefCell<Option<Arc<Apply>>> = const { RefCell::new(None) };
}

/// The preview of the request being polled, if it is one.
pub(crate) fn current() -> Option<Arc<Preview>> {
    CURRENT.with_borrow(|current| current.clone())
}

/// The apply of the request being polled, if it is one: a write sent with its preview's
/// token.
pub(crate) fn applying() -> Option<Arc<Apply>> {
    APPLYING.with_borrow(|current| current.clone())
}

type EventsOf = dyn Fn() -> BoxFuture<Result<Vec<StoredEvent>, RepoError>> + Send;
type FinishWith = dyn FnOnce(bool, Option<PreviewToken>) -> BoxFuture<Result<(), RepoError>> + Send;

/// A store's open preview or apply (`Storage::begin_preview`, `begin_apply`): what reads
/// back the events its writes recorded, and what ends it (committing or not, then storing a
/// token if one is given). Boxed, so this module knows no storage; built by each host for
/// its concrete storage, where the futures are provably `Send`.
pub struct OpenPreview {
    events: Box<EventsOf>,
    finish: Box<FinishWith>,
}

impl OpenPreview {
    pub fn new(
        events: impl Fn() -> BoxFuture<Result<Vec<StoredEvent>, RepoError>> + Send + 'static,
        finish: impl FnOnce(bool, Option<PreviewToken>) -> BoxFuture<Result<(), RepoError>>
        + Send
        + 'static,
    ) -> Self {
        Self {
            events: Box::new(events),
            finish: Box::new(finish),
        }
    }
}

/// What `stores` recorded, in the order of the stores. The reads start here, so no
/// borrow of `stores` is held across an await (`OpenPreview` is `Send`, not `Sync`).
fn events_of(
    stores: &[OpenPreview],
) -> impl Future<Output = Result<Vec<StoredEvent>, RepoError>> + Send + use<> {
    let reads: Vec<_> = stores.iter().map(|store| (store.events)()).collect();
    async move {
        let mut events = Vec::new();
        for read in reads {
            events.append(&mut read.await?);
        }
        Ok(events)
    }
}

/// Ends every store, committed or not, whatever happened to the one before; `token` is
/// stored by the first.
async fn finish_all(
    stores: Vec<OpenPreview>,
    commit: bool,
    mut token: Option<PreviewToken>,
) -> Result<(), RepoError> {
    let mut failed = None;
    for store in stores {
        if let Err(e) = (store.finish)(commit, token.take()).await {
            failed = Some(e);
        }
    }
    failed.map_or(Ok(()), Err)
}

/// What issuing a token needs from the host, given when the request's ports open
/// (`AppState::public_ports`).
#[derive(Clone)]
pub(crate) struct TokenHost {
    pub clock: HostClock,
    pub random: HostRandom,
    pub lifetime: Duration,
}

/// A token made for a preview: the record stored, and what the caller is shown.
struct Issued {
    record: PreviewToken,
    token: String,
    expires_at: String,
}

/// A token applying `request` as `actor` previewed it: its `ids`, events digested to
/// `digest`.
fn issue(
    host: &TokenHost,
    actor: Actor,
    request: [u8; 32],
    ids: Vec<Uuid>,
    digest: String,
) -> Result<Issued, ApiError> {
    let token = NewToken::generate(TokenKind::PreviewToken, &host.random);
    let expires_at = host.clock.now().plus(host.lifetime);
    Ok(Issued {
        record: PreviewToken {
            hash: *token.hash().as_bytes(),
            actor,
            request,
            ids,
            digest,
            expires_at,
            used_at: None,
        },
        token: token.expose().to_string(),
        expires_at: crate::routes::rfc3339(expires_at)?,
    })
}

/// The digest of what a write recorded, which applying it must match: SHA-256 over its
/// events in order, each as its type, version, subject and data, with every value that is
/// an RFC 3339 timestamp blanked (times move between a preview and its apply; nothing
/// else should). As `sha256:<hex>`.
pub(crate) fn digest(events: &[StoredEvent]) -> Result<String, ApiError> {
    let mut list = Vec::with_capacity(events.len());
    for event in events {
        let envelope = crate::webhooks::EventEnvelope::new(event)?;
        let mut data = event.data.clone();
        blank_times(&mut data);
        list.push(serde_json::json!({
            "type": envelope.event_type,
            "version": envelope.version,
            "subject": envelope.subject.id,
            "data": data,
        }));
    }
    let mut canonical = Vec::new();
    crate::extract::write_canonical(&Value::Array(list), &mut canonical);
    let hash: [u8; 32] = Sha256::digest(&canonical).into();
    let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    Ok(format!("sha256:{hex}"))
}

/// Blanks every string in `value` that is an RFC 3339 timestamp.
fn blank_times(value: &mut Value) {
    match value {
        Value::String(text) if text.parse::<jiff::Timestamp>().is_ok() => text.clear(),
        Value::Array(items) => items.iter_mut().for_each(blank_times),
        Value::Object(map) => map.values_mut().for_each(blank_times),
        _ => {}
    }
}

/// One preview request's record: its stores' open previews, what its effects would have
/// been, the first one it couldn't preview, the IDs it was handed, who it is (for
/// redacting its events and binding its token) and the request's fingerprint.
#[derive(Default)]
pub struct Preview {
    request: [u8; 32],
    stores: Mutex<Vec<OpenPreview>>,
    host: Mutex<Option<TokenHost>>,
    emails: Mutex<Vec<PreviewEmail>>,
    calls: Mutex<Vec<PreviewCall>>,
    blocked: Mutex<Option<BlockedBy>>,
    ids: Mutex<Vec<Uuid>>,
    viewer: Mutex<Option<Viewer>>,
}

/// Who asked for the preview, for showing its events as the caller may see them: a staff
/// session sees everything; anyone else what its scopes allow, custom field entries as
/// `redact::changes_visible_to` judges them with the definitions as they were before the
/// write.
pub(crate) struct Viewer {
    pub ctx: Ctx,
    pub fields: Option<(CallerKind, Vec<CustomFieldDefinition>)>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // A poisoned lock still holds whole values: each is pushed or replaced at once.
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Why a call wasn't sent: the request is a preview. What the caller sees is the preview's
/// `blocked_by`, not this.
pub(crate) const NOT_SENT: &str = "not sent: the request is a preview";

impl Preview {
    fn for_request(request: [u8; 32]) -> Self {
        Self {
            request,
            ..Self::default()
        }
    }

    /// Keeps a store's open preview, to end once the handler is done.
    pub(crate) fn hold(&self, store: OpenPreview, host: TokenHost) {
        lock(&self.stores).push(store);
        lock(&self.host).get_or_insert(host);
    }

    pub(crate) fn set_viewer(&self, viewer: Viewer) {
        *lock(&self.viewer) = Some(viewer);
    }

    pub(crate) fn email(&self, email: PreviewEmail) {
        lock(&self.emails).push(email);
    }

    pub(crate) fn id(&self, id: Uuid) {
        lock(&self.ids).push(id);
    }

    /// Notes the first effect the preview couldn't run; the command fails there.
    pub(crate) fn block(&self, by: BlockedBy) {
        lock(&self.blocked).get_or_insert(by);
    }

    /// Whether a call to an app may go out: only to a read-only extension point, which is
    /// recorded. Anything else is blocked.
    pub(crate) fn allows_call(&self, purpose: &CallPurpose) -> bool {
        match purpose {
            CallPurpose::Extension {
                point,
                app,
                read_only: true,
            } => {
                lock(&self.calls).push(PreviewCall {
                    point,
                    app: app.clone(),
                });
                true
            }
            CallPurpose::Extension { point, app, .. } => {
                self.block(BlockedBy::ExtensionCall {
                    point,
                    app: app.clone(),
                });
                false
            }
            CallPurpose::Other => {
                self.block(BlockedBy::OutboundCall);
                false
            }
        }
    }

    /// Ends the stores' previews, then answers for the handler's `response`.
    async fn finish(&self, response: Response) -> Response {
        let stores = std::mem::take(&mut *lock(&self.stores));
        let events = events_of(&stores).await;
        let (parts, body) = response.into_parts();
        let bytes = match axum::body::to_bytes(body, MAX_RESPONSE_BYTES).await {
            Ok(bytes) => bytes,
            Err(e) => {
                let _ = finish_all(stores, false, None).await;
                return ApiError::internal(format!("a previewed response: {e}")).into_response();
            }
        };
        let blocked = lock(&self.blocked).take();
        if blocked.is_none() && !parts.status.is_success() {
            // The write's own failure is the answer; ending the stores can't change it.
            let _ = finish_all(stores, false, None).await;
            return Response::from_parts(parts, Body::from(bytes));
        }
        let answered = events
            .map_err(|e| ApiError::internal(format!("ending a preview: {e}")))
            .and_then(|events| self.answer(parts.status, &bytes, &events, blocked));
        let (preview, ids, digest, token) = match answered {
            Ok(answered) => answered,
            Err(e) => {
                let _ = finish_all(stores, false, None).await;
                return e.into_response();
            }
        };
        if let Err(e) = finish_all(stores, false, token).await {
            return ApiError::internal(format!("ending a preview: {e}")).into_response();
        }
        let mut answer = (StatusCode::OK, axum::Json(PreviewResponse { preview })).into_response();
        answer.extensions_mut().insert(PreviewIds(ids));
        answer.extensions_mut().insert(PreviewDigest(digest));
        answer
    }

    /// The preview of a write that answered `status` with `bytes` and recorded `events`;
    /// the IDs it was handed; its events' digest; and, when it ran to its end for a known
    /// caller, the token that applies it.
    fn answer(
        &self,
        status: StatusCode,
        bytes: &[u8],
        events: &[StoredEvent],
        blocked: Option<BlockedBy>,
    ) -> Result<(PreviewBody, Vec<Uuid>, String, Option<PreviewToken>), ApiError> {
        let viewer = lock(&self.viewer).take();
        let emails = std::mem::take(&mut *lock(&self.emails));
        let calls = std::mem::take(&mut *lock(&self.calls));
        let complete = blocked.is_none();
        let mut preview = PreviewBody::build(
            status,
            bytes,
            events,
            viewer.as_ref(),
            emails,
            calls,
            blocked,
        )?;
        let ids = std::mem::take(&mut *lock(&self.ids));
        let host = lock(&self.host).clone();
        let digest = digest(events)?;
        let token = match (complete, host, &viewer) {
            (true, Some(host), Some(viewer)) => {
                let issued = issue(
                    &host,
                    viewer.ctx.actor(),
                    self.request,
                    ids.clone(),
                    digest.clone(),
                )?;
                preview.token = Some(issued.token);
                preview.expires_at = Some(issued.expires_at);
                Some(issued.record)
            }
            _ => None,
        };
        Ok((preview, ids, digest, token))
    }
}

/// What a write sent with its preview's token checks: who previewed, and what its events
/// came to.
struct Claimed {
    digest: String,
}

/// One apply request's record (a write sent with `Hypha-Preview-Token`): its stores' open
/// transactions, the token and the request's fingerprint, the preview's IDs still to hand
/// out and every ID handed, what the claimed token says, and who the caller is.
pub(crate) struct Apply {
    token: TokenHash,
    request: [u8; 32],
    stores: Mutex<Vec<OpenPreview>>,
    host: Mutex<Option<TokenHost>>,
    queue: Mutex<VecDeque<Uuid>>,
    handed: Mutex<Vec<Uuid>>,
    claimed: Mutex<Option<Claimed>>,
    viewer: Mutex<Option<Viewer>>,
}

impl Apply {
    fn new(token: TokenHash, request: [u8; 32]) -> Self {
        Self {
            token,
            request,
            stores: Mutex::default(),
            host: Mutex::default(),
            queue: Mutex::default(),
            handed: Mutex::default(),
            claimed: Mutex::default(),
            viewer: Mutex::default(),
        }
    }

    /// Keeps a store's open transaction, to end once the handler is done.
    pub(crate) fn hold(&self, store: OpenPreview, host: TokenHost) {
        lock(&self.stores).push(store);
        lock(&self.host).get_or_insert(host);
    }

    pub(crate) fn set_viewer(&self, viewer: Viewer) {
        *lock(&self.viewer) = Some(viewer);
    }

    /// The next ID for the command: the preview's, in order, then `fresh` ones.
    pub(crate) fn next_id(&self, fresh: impl FnOnce() -> Uuid) -> Uuid {
        let id = lock(&self.queue).pop_front().unwrap_or_else(fresh);
        lock(&self.handed).push(id);
        id
    }

    /// Checks the token for the caller `ctx` and marks it used, through `ports` (in the
    /// apply's transaction: a write that rolls back leaves it unused). Unknown, expired,
    /// another caller's or another request's: `preview_token_invalid`; used:
    /// `preview_used`.
    pub(crate) async fn claim<P: HasPreviewTokens>(
        &self,
        ports: &P,
        ctx: &Ctx,
        now: Timestamp,
    ) -> Result<(), ApiError> {
        let hash = self.token.as_bytes();
        let Some(token) = ports.preview_tokens().get(hash).await? else {
            return Err(ApiError::preview_token_invalid("it's unknown"));
        };
        if token.actor != ctx.actor() {
            return Err(ApiError::preview_token_invalid("it's another caller's"));
        }
        if token.request != self.request {
            return Err(ApiError::preview_token_invalid(
                "it previewed another request",
            ));
        }
        if token.used_at.is_some() {
            return Err(ApiError::preview_used());
        }
        if token.expires_at <= now {
            return Err(ApiError::preview_token_invalid("it expired"));
        }
        if !ports.preview_tokens().mark_used(hash, now).await? {
            return Err(ApiError::preview_used());
        }
        *lock(&self.queue) = token.ids.into();
        *lock(&self.claimed) = Some(Claimed {
            digest: token.digest,
        });
        Ok(())
    }

    /// Ends the stores' transactions: committed when the write succeeded and recorded what
    /// its preview did; otherwise rolled back, answering the write's failure or
    /// `preview_stale` with what it would do now.
    async fn finish(&self, response: Response) -> Response {
        let stores = std::mem::take(&mut *lock(&self.stores));
        let claimed = lock(&self.claimed).take();
        if !response.status().is_success() {
            let _ = finish_all(stores, false, None).await;
            return response;
        }
        let Some(claimed) = claimed else {
            let _ = finish_all(stores, false, None).await;
            return ApiError::preview_token_invalid("this route takes no credentials")
                .into_response();
        };
        let events = match events_of(&stores).await {
            Ok(events) => events,
            Err(e) => {
                let _ = finish_all(stores, false, None).await;
                return ApiError::internal(format!("applying a preview: {e}")).into_response();
            }
        };
        let now = match digest(&events) {
            Ok(now) => now,
            Err(e) => {
                let _ = finish_all(stores, false, None).await;
                return e.into_response();
            }
        };
        if now == claimed.digest {
            return match finish_all(stores, true, None).await {
                Ok(()) => response,
                Err(e) => ApiError::internal(format!("committing an apply: {e}")).into_response(),
            };
        }
        let (parts, body) = response.into_parts();
        let stale = match axum::body::to_bytes(body, MAX_RESPONSE_BYTES).await {
            Ok(bytes) => self.stale(parts.status, &bytes, &events, now),
            Err(e) => Err(ApiError::internal(format!("an applied response: {e}"))),
        };
        match stale {
            Ok((preview, token)) => match finish_all(stores, false, token).await {
                Ok(()) => match serde_json::to_value(&preview) {
                    Ok(preview) => ApiError::preview_stale(preview).into_response(),
                    Err(e) => ApiError::internal(e).into_response(),
                },
                Err(e) => ApiError::internal(format!("ending an apply: {e}")).into_response(),
            },
            Err(e) => {
                let _ = finish_all(stores, false, None).await;
                e.into_response()
            }
        }
    }

    /// What a stale apply did instead, as a preview, with a token for it: the write as it
    /// would be now. Its effects outside the database weren't recorded (they ran), so its
    /// `emails` and `calls` are empty.
    fn stale(
        &self,
        status: StatusCode,
        bytes: &[u8],
        events: &[StoredEvent],
        digest: String,
    ) -> Result<(PreviewBody, Option<PreviewToken>), ApiError> {
        let viewer = lock(&self.viewer).take();
        let mut preview = PreviewBody::build(
            status,
            bytes,
            events,
            viewer.as_ref(),
            Vec::new(),
            Vec::new(),
            None,
        )?;
        let host = lock(&self.host).clone();
        let ids = std::mem::take(&mut *lock(&self.handed));
        let token = match (host, &viewer) {
            (Some(host), Some(viewer)) => {
                let issued = issue(&host, viewer.ctx.actor(), self.request, ids, digest)?;
                preview.token = Some(issued.token);
                preview.expires_at = Some(issued.expires_at);
                Some(issued.record)
            }
            _ => None,
        };
        Ok((preview, token))
    }
}

/// The IDs a preview was handed, in order: on the preview's response's extensions, not
/// in its body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewIds(pub Vec<Uuid>);

/// The digest of a preview's events (`digest`), on its response's extensions: what a
/// proposal keeps to apply it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewDigest(pub String);

/// Polls `inner` with `preview` or `apply` current (`current`, `applying`).
struct Scoped<F> {
    preview: Option<Arc<Preview>>,
    apply: Option<Arc<Apply>>,
    inner: Pin<Box<F>>,
}

impl<F: Future> Future for Scoped<F> {
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<F::Output> {
        let this = self.get_mut();
        let outer_preview = CURRENT.replace(this.preview.clone());
        let outer_apply = APPLYING.replace(this.apply.clone());
        let polled = this.inner.as_mut().poll(cx);
        CURRENT.set(outer_preview);
        APPLYING.set(outer_apply);
        polled
    }
}

/// `inner` with neither a preview nor an apply current: a request sent in process
/// (`internal::dispatch`) is its own, whatever the request sending it is.
pub(crate) fn detached<F: Future>(inner: F) -> impl Future<Output = F::Output> {
    Scoped {
        preview: None,
        apply: None,
        inner: Box::pin(inner),
    }
}

/// What `Hypha-Preview` asks for: `Some(true)` a preview, `Some(false)` none (`0`), `None`
/// a value it can't be.
fn asks_for_preview(request: &Request) -> Option<bool> {
    match request.headers().get(PREVIEW_HEADER) {
        None => Some(false),
        Some(value) => match value.to_str().map(str::trim) {
            Ok("1") => Some(true),
            Ok("0") => Some(false),
            _ => None,
        },
    }
}

/// The token in `Hypha-Preview-Token`: `None` without the header, `Err` for a value that
/// isn't a preview token.
fn sent_token(request: &Request) -> Result<Option<TokenHash>, ApiError> {
    let Some(value) = request.headers().get(PREVIEW_TOKEN_HEADER) else {
        return Ok(None);
    };
    match value.to_str().map(str::trim) {
        Ok(token) if TokenKind::of(token) == Some(TokenKind::PreviewToken) => {
            Ok(Some(TokenHash::of(token)))
        }
        _ => Err(ApiError::preview_token_invalid(
            "it isn't a preview token (hypha_pv_…)",
        )),
    }
}

/// Whether a request is an admin write: any method but `GET`, `HEAD` and `OPTIONS` under
/// `/admin/`.
fn is_admin_write(request: &Request) -> bool {
    !matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    ) && request.uri().path().starts_with("/admin/")
}

/// `request` with its body read, and its fingerprint (`extract::fingerprint`).
async fn fingerprinted(request: Request) -> Result<(Request, [u8; 32]), ApiError> {
    let (parts, body) = request.into_parts();
    let bytes: Bytes = axum::body::to_bytes(body, MAX_REQUEST_BYTES)
        .await
        .map_err(|_| ApiError::body_too_large(MAX_REQUEST_BYTES))?;
    let fingerprint = crate::extract::fingerprint(
        parts.method.as_str(),
        parts.uri.path(),
        parts.uri.query().unwrap_or(""),
        &bytes,
    );
    Ok((Request::from_parts(parts, Body::from(bytes)), fingerprint))
}

/// The middleware that previews admin writes sent with `Hypha-Preview: 1` and applies those
/// sent with `Hypha-Preview-Token`; reads ignore both. Hosts layer it over every route,
/// their spores' included (`with_previews`).
pub async fn middleware(request: Request, next: Next) -> Response {
    if !is_admin_write(&request) {
        return next.run(request).await;
    }
    let token = match sent_token(&request) {
        Ok(token) => token,
        Err(e) => return e.into_response(),
    };
    match (asks_for_preview(&request), token) {
        (None, _) => ApiError::invalid_preview_header().into_response(),
        (Some(true), Some(_)) => ApiError::preview_with_token().into_response(),
        (Some(false), None) => next.run(request).await,
        (Some(true), None) => {
            let (request, fingerprint) = match fingerprinted(request).await {
                Ok(fingerprinted) => fingerprinted,
                Err(e) => return e.into_response(),
            };
            let preview = Arc::new(Preview::for_request(fingerprint));
            let response = Scoped {
                preview: Some(Arc::clone(&preview)),
                apply: None,
                inner: Box::pin(next.run(request)),
            }
            .await;
            preview.finish(response).await
        }
        (Some(false), Some(token)) => {
            let (request, fingerprint) = match fingerprinted(request).await {
                Ok(fingerprinted) => fingerprinted,
                Err(e) => return e.into_response(),
            };
            let apply = Arc::new(Apply::new(token, fingerprint));
            let response = Scoped {
                preview: None,
                apply: Some(Arc::clone(&apply)),
                inner: Box::pin(next.run(request)),
            }
            .await;
            apply.finish(response).await
        }
    }
}

/// `router` with admin writes previewable and applicable as previewed (`middleware`).
pub fn with_previews<S: Clone + Send + Sync + 'static>(router: axum::Router<S>) -> axum::Router<S> {
    router.layer(axum::middleware::from_fn(middleware))
}

/// What a previewed write answers.
#[derive(Serialize, utoipa::ToSchema)]
pub struct PreviewResponse {
    pub preview: PreviewBody,
}

/// What the write would have done; nothing of it was kept.
#[derive(Serialize, utoipa::ToSchema)]
pub struct PreviewBody {
    /// Whether the write ran to its end. `false`: it reached an effect outside the
    /// database a preview can't run (`blocked_by`), and stopped there; what it did up to
    /// then is listed.
    pub complete: bool,
    /// What the route would have answered: its status and body, redacted as it would
    /// be. `null` when the write stopped at a blocked effect.
    pub response: Option<PreviewedResponse>,
    /// The events it would have recorded, in order, as the caller may see them.
    pub events: Vec<PreviewEvent>,
    /// The emails it would have sent while answering. Emails sent later because of its
    /// events (an order's confirmation) aren't listed.
    pub emails: Vec<PreviewEmail>,
    /// The read-only extension points it called (`shipping.rates`, `search.query`,
    /// `payments.status`): those are called for real.
    pub calls: Vec<PreviewCall>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked_by: Option<BlockedBy>,
    /// Sent back as `Hypha-Preview-Token` with the same request (method, path, query and
    /// body) by the same caller, it applies exactly this: the same IDs, and only if the
    /// write records the same events (else `409 preview_stale`). Once. Absent when the
    /// preview isn't `complete`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    /// When `token` stops working (RFC 3339).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
}

impl PreviewBody {
    /// The preview of a write that answered `status` with `bytes` and recorded `events`,
    /// shown as `viewer` may see it.
    fn build(
        status: StatusCode,
        bytes: &[u8],
        events: &[StoredEvent],
        viewer: Option<&Viewer>,
        emails: Vec<PreviewEmail>,
        calls: Vec<PreviewCall>,
        blocked: Option<BlockedBy>,
    ) -> Result<Self, ApiError> {
        let events = events
            .iter()
            .map(|event| PreviewEvent::new(event, viewer))
            .collect::<Result<Vec<_>, _>>()?;
        let removes_email = viewer.is_none_or(|v| Redaction::of(&v.ctx).removes_email());
        let emails = emails
            .into_iter()
            .map(|email| email.redacted(removes_email))
            .collect();
        let response = match blocked {
            // What a write answers after a blocked effect is that effect's failure, not
            // what it would have answered.
            Some(_) if !status.is_success() => None,
            _ => Some(PreviewedResponse {
                status: status.as_u16(),
                body: match bytes.is_empty() {
                    true => Value::Null,
                    false => serde_json::from_slice(bytes).unwrap_or(Value::Null),
                },
            }),
        };
        Ok(Self {
            complete: blocked.is_none(),
            response,
            events,
            emails,
            calls,
            blocked_by: blocked,
            token: None,
            expires_at: None,
        })
    }
}

/// The route's answer, had the write run.
#[derive(Serialize, utoipa::ToSchema)]
pub struct PreviewedResponse {
    pub status: u16,
    #[schema(value_type = Object)]
    pub body: Value,
}

/// An event the write would have recorded: as webhooks carry it, without the ID, actor and
/// time it would have had.
#[derive(Serialize, utoipa::ToSchema)]
pub struct PreviewEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub version: u16,
    /// The prefixed ID of what it's about (`prod_…`).
    pub subject: String,
    #[schema(value_type = Object)]
    pub data: Value,
}

impl PreviewEvent {
    fn new(event: &StoredEvent, viewer: Option<&Viewer>) -> Result<Self, ApiError> {
        let envelope = crate::webhooks::EventEnvelope::new(event)?;
        let data = match viewer {
            Some(viewer) if viewer.ctx.is_staff() => event.data.clone(),
            Some(viewer) => visible_data(event, viewer),
            // Nobody known (a route without credentials): no personal data.
            None => crate::redact::event_data(|_| false, &event.data),
        };
        Ok(Self {
            event_type: envelope.event_type,
            version: envelope.version,
            subject: envelope.subject.id,
            data,
        })
    }
}

/// `event`'s data as a caller other than a staff session sees it (`GET /admin/events/{id}`).
fn visible_data(event: &StoredEvent, viewer: &Viewer) -> Value {
    let mut data = event.data.clone();
    if crate::redact::is_custom_fields_event(&event.event_type) {
        data = match &viewer.fields {
            Some((kind, definitions)) => crate::redact::changes_visible_to(
                &FieldCaller::new(&viewer.ctx, kind),
                definitions,
                &event.event_type,
                &data,
            ),
            None => None,
        }
        .unwrap_or_else(|| {
            let mut empty = data.clone();
            empty["changes"] = Value::Array(Vec::new());
            empty
        });
    }
    crate::redact::event_data(|s| viewer.ctx.has_scope(s), &data)
}

/// An email the write would have sent.
#[derive(Serialize, utoipa::ToSchema)]
pub struct PreviewEmail {
    /// The template's key (`staff_invite`).
    pub template: String,
    /// Absent without `customer_data:email` (listed in `redacted`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    pub subject: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub redacted: Vec<&'static str>,
}

impl PreviewEmail {
    pub(crate) fn new(email: &hypha_notifications::OutgoingEmail) -> Self {
        Self {
            template: email.template.as_str().to_string(),
            to: Some(email.to.as_str().to_string()),
            subject: email.subject.clone(),
            redacted: Vec::new(),
        }
    }

    fn redacted(mut self, removes_email: bool) -> Self {
        if removes_email {
            self.to = None;
            self.redacted.push("to");
        }
        self
    }
}

/// A read-only extension point the write called.
#[derive(Serialize, utoipa::ToSchema)]
pub struct PreviewCall {
    /// `shipping.rates`, `search.query` or `payments.status`.
    pub point: &'static str,
    /// The app that answered (`app_rates`).
    pub app: String,
}

/// The effect outside the database a preview couldn't run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BlockedBy {
    /// A call to an app's extension point that changes something (`payments.refund`).
    ExtensionCall { point: &'static str, app: String },
    /// Any other call out (a manifest, a spore's provider).
    OutboundCall,
    /// The media storage (checking or removing a file).
    MediaStorage,
    /// A URL to upload a file to the media storage.
    MediaUpload,
    /// Applying an agent's proposal: its write is its own request, previewed when it was
    /// proposed.
    Proposal,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(method: Method, path: &str, header: Option<&str>) -> Request {
        let mut builder = axum::http::Request::builder().method(method).uri(path);
        if let Some(value) = header {
            builder = builder.header(PREVIEW_HEADER, value);
        }
        builder.body(Body::empty()).unwrap()
    }

    #[test]
    fn only_admin_writes_are_previewed() {
        assert!(is_admin_write(&request(
            Method::POST,
            "/admin/products",
            None
        )));
        assert!(is_admin_write(&request(
            Method::DELETE,
            "/admin/products/x",
            None
        )));
        assert!(!is_admin_write(&request(
            Method::GET,
            "/admin/products",
            None
        )));
        assert!(!is_admin_write(&request(
            Method::POST,
            "/storefront/carts",
            None
        )));
        assert!(!is_admin_write(&request(
            Method::POST,
            "/administrators",
            None
        )));
    }

    #[test]
    fn the_header_is_one_or_zero() {
        let asks = |value| asks_for_preview(&request(Method::POST, "/admin/x", value));
        assert_eq!(asks(None), Some(false));
        assert_eq!(asks(Some("1")), Some(true));
        assert_eq!(asks(Some(" 1 ")), Some(true));
        assert_eq!(asks(Some("0")), Some(false));
        assert_eq!(asks(Some("true")), None);
        assert_eq!(asks(Some("")), None);
    }

    #[test]
    fn the_preview_is_current_only_while_its_future_is_polled() {
        let preview = Arc::new(Preview::default());
        let seen = pollster::block_on(Scoped {
            preview: Some(Arc::clone(&preview)),
            apply: None,
            inner: Box::pin(async { current().is_some() && applying().is_none() }),
        });
        assert!(seen);
        assert!(current().is_none());
    }

    fn event(data: Value) -> StoredEvent {
        StoredEvent {
            seq: 1,
            id: Uuid::from_u128(1),
            event_type: "order.noted".to_string(),
            version: 1,
            subject_kind: "order".to_string(),
            subject_id: Uuid::from_u128(2),
            subject_version: None,
            actor: hypha_core::Actor::System,
            occurred_at: hypha_core::Timestamp::UNIX_EPOCH,
            data,
        }
    }

    fn viewer(actor: hypha_core::Actor, scopes: Vec<hypha_core::Scope>) -> Viewer {
        Viewer {
            ctx: Ctx::new(actor, scopes),
            fields: None,
        }
    }

    #[test]
    fn events_show_personal_data_only_to_who_may_see_it() {
        use hypha_core::{Actor, ApiKeyId, Scope, UserId};
        let data = serde_json::json!({"email": "ana@example.hr", "note": "gift"});
        let staff = viewer(Actor::User(UserId::from_uuid(Uuid::from_u128(3))), vec![]);
        let key = ApiKeyId::from_uuid(Uuid::from_u128(4));
        let plain = viewer(Actor::ApiKey(key), vec![Scope::OrdersRead]);
        let trusted = viewer(Actor::ApiKey(key), vec![Scope::CustomerDataEmail]);
        let shown = |viewer: Option<&Viewer>| {
            PreviewEvent::new(&event(data.clone()), viewer)
                .unwrap()
                .data
        };
        assert_eq!(shown(Some(&staff)), data);
        assert_eq!(shown(Some(&trusted)), data);
        assert_eq!(shown(Some(&plain)).get("email"), None);
        assert_eq!(shown(Some(&plain))["note"], "gift");
        assert_eq!(shown(None).get("email"), None, "nobody known sees none");
        let preview = PreviewEvent::new(&event(data.clone()), None).unwrap();
        assert_eq!(
            (preview.event_type.as_str(), preview.subject.as_str()),
            ("order.noted", format!("ord_{:032x}", 2).as_str())
        );
    }

    #[test]
    fn the_first_blocked_effect_is_the_one_reported() {
        let preview = Preview::default();
        assert!(!preview.allows_call(&CallPurpose::Extension {
            point: "payments.refund",
            app: "app_pay".to_string(),
            read_only: false,
        }));
        assert!(!preview.allows_call(&CallPurpose::Other));
        assert!(preview.allows_call(&CallPurpose::Extension {
            point: "shipping.rates",
            app: "app_rates".to_string(),
            read_only: true,
        }));
        assert_eq!(
            *lock(&preview.blocked),
            Some(BlockedBy::ExtensionCall {
                point: "payments.refund",
                app: "app_pay".to_string()
            })
        );
        assert_eq!(lock(&preview.calls).len(), 1);
    }
}
