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
//! digest of what it did (its events, and the rows of aggregates it wrote with the
//! versions it found). Sending the same write with `Hypha-Preview-Token` applies it:
//!
//! - its ports run in one transaction too (`Storage::begin_apply`); `Caller::authenticate`
//!   checks the token against the caller and marks it used, in that transaction;
//! - the command is handed the preview's IDs first (`HostIds`), so what it creates is what
//!   the preview showed;
//! - once the handler is done, what its writes did is digested like the preview's: equal,
//!   everything commits; different (something it writes changed meanwhile), it's
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
    Actor, Clock, Ctx, HasPreviewTokens, PreviewToken, PreviewTokenRepo, RepoError, RowChange,
    StoredEvent, Timestamp, WriteRecord,
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
    /// The preview or apply whose request is being polled (`Scoped`).
    static CURRENT: RefCell<Option<Arc<Run>>> = const { RefCell::new(None) };
}

/// The preview or apply of the request being polled, if it is one.
pub(crate) fn current() -> Option<Arc<Run>> {
    CURRENT.with_borrow(|current| current.clone())
}

/// Whether the request being polled is a preview (not an apply, not a plain write).
pub(crate) fn previewing() -> bool {
    current().is_some_and(|run| run.apply.is_none())
}

type RecordOf = dyn Fn() -> BoxFuture<Result<WriteRecord, RepoError>> + Send;
type FinishWith = dyn FnOnce(bool, Option<PreviewToken>) -> BoxFuture<Result<(), RepoError>> + Send;

/// A store's open preview or apply (`Storage::begin_preview`, `begin_apply`): what reads
/// back what its writes did (the events recorded, the rows written), and what ends it
/// (committing or not, then storing a token if one is given). Boxed, so this module knows
/// no storage; built by each host for its concrete storage, where the futures are
/// provably `Send`.
pub struct OpenPreview {
    recorded: Box<RecordOf>,
    finish: Box<FinishWith>,
}

impl OpenPreview {
    pub fn new(
        recorded: impl Fn() -> BoxFuture<Result<WriteRecord, RepoError>> + Send + 'static,
        finish: impl FnOnce(bool, Option<PreviewToken>) -> BoxFuture<Result<(), RepoError>>
        + Send
        + 'static,
    ) -> Self {
        Self {
            recorded: Box::new(recorded),
            finish: Box::new(finish),
        }
    }
}

/// What `stores` did, in the order of the stores. The reads start here, so no borrow of
/// `stores` is held across an await (`OpenPreview` is `Send`, not `Sync`).
fn recorded_by(
    stores: &[OpenPreview],
) -> impl Future<Output = Result<WriteRecord, RepoError>> + Send + use<> {
    let reads: Vec<_> = stores.iter().map(|store| (store.recorded)()).collect();
    async move {
        let mut all = WriteRecord::default();
        for read in reads {
            let mut one = read.await?;
            all.events.append(&mut one.events);
            all.rows.append(&mut one.rows);
        }
        Ok(all)
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

/// A token applying `request` as `actor` previewed it: its `ids`, what it did digested
/// to `digest`.
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

/// The digest of what a write did, which applying it must match: SHA-256 over its events
/// in order, each as its type, version, subject and data, with every time blanked
/// (`blank_times`: times move between a preview and its apply; nothing else should), and
/// over the rows of aggregates it wrote, in order, each with the version it
/// found (`Inserted` for a new one), in a fixed order. The rows are what catch a record changed since the
/// preview whose change no event payload would show (`product.updated` names the fields
/// changed, not their old values): whatever the write read and wrote again must be as it
/// was. As `sha256:<hex>`.
pub(crate) fn digest(record: &WriteRecord) -> Result<String, ApiError> {
    let mut list = Vec::with_capacity(record.events.len());
    for event in &record.events {
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
    let mut rows: Vec<_> = record
        .rows
        .iter()
        .map(|row| {
            let (change, found) = match row.change {
                RowChange::Inserted => ("insert", None),
                RowChange::Updated(v) => ("update", Some(v)),
                RowChange::Deleted(v) => ("delete", Some(v)),
            };
            (row.kind.as_str(), row.key.as_str(), change, found)
        })
        .collect();
    // The order rows are written in follows the database's plan (an `UPDATE … FROM
    // unnest`), which a preview and its apply needn't share.
    rows.sort();
    let rows = rows
        .into_iter()
        .map(|(kind, key, change, found)| serde_json::json!([kind, key, change, found]))
        .collect();
    let all = serde_json::json!({"events": list, "rows": Value::Array(rows)});
    let mut canonical = Vec::new();
    crate::extract::write_canonical(&all, &mut canonical);
    let hash: [u8; 32] = Sha256::digest(&canonical).into();
    let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    Ok(format!("sha256:{hex}"))
}

/// Blanks every time in `value`: a string that is an RFC 3339 timestamp, and a number
/// under a key ending in `_at` (microseconds since the epoch, as `Timestamp` counts).
fn blank_times(value: &mut Value) {
    match value {
        Value::String(text) if text.parse::<jiff::Timestamp>().is_ok() => text.clear(),
        Value::Array(items) => items.iter_mut().for_each(blank_times),
        Value::Object(map) => {
            for (key, value) in map.iter_mut() {
                match value {
                    Value::Number(_) if key.ends_with("_at") => *value = Value::Null,
                    _ => blank_times(value),
                }
            }
        }
        _ => {}
    }
}

/// An effect outside the database held until the write commits (an email, background
/// work): run after an apply's COMMIT; dropped with a write that rolls back, and always
/// in a preview.
pub(crate) type Deferred = Box<dyn FnOnce() -> BoxFuture<()> + Send>;

/// One previewed or applied request's record: its stores' open transactions, who the
/// caller is (for redacting its events and binding its token), the request's
/// fingerprint, every ID handed out, and its effects outside the database: what they came
/// to and those held until a commit.
///
/// An apply (`Hypha-Preview-Token`) "applies what was previewed, or nothing", effects
/// included. Its IDs are the preview's first. Emails and background work are held and
/// happen only once the write commits. A call out is allowed only if its preview could
/// have made it: its preview was complete, so it made no call but to read-only extension
/// points (which are made again, as in a preview) and touched no media storage; anything
/// else is blocked before it goes out and the apply answers stale.
pub(crate) struct Run {
    request: [u8; 32],
    /// `Some` for an apply.
    apply: Option<Applying>,
    stores: Mutex<Vec<OpenPreview>>,
    host: Mutex<Option<TokenHost>>,
    handed: Mutex<Vec<Uuid>>,
    viewer: Mutex<Option<Viewer>>,
    effects: Effects,
    deferred: Mutex<Vec<Deferred>>,
}

/// What an apply has beyond a preview: the token sent, the preview's IDs still to hand
/// out, and the digest the token holds once claimed.
struct Applying {
    token: TokenHash,
    queue: Mutex<VecDeque<Uuid>>,
    claimed: Mutex<Option<String>>,
}

/// What a preview's or an apply's effects outside the database came to: the emails it
/// would send (an apply holds them until it commits), the read-only extension points it
/// called, and the first effect it couldn't run.
#[derive(Default)]
struct Effects {
    emails: Mutex<Vec<PreviewEmail>>,
    calls: Mutex<Vec<PreviewCall>>,
    blocked: Mutex<Option<BlockedBy>>,
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
const NOT_SENT: &str = "not sent: the request is a preview";

/// Why a call wasn't sent while applying a preview: its preview made no such call.
const NOT_PREVIEWED: &str = "not sent: the preview being applied made no such call";

/// How a request's stores end, and what it answers.
enum Ending {
    /// Rolled back, answering the write's own answer (a failure, a replay) or an error:
    /// ending the stores can't change it.
    Pass(Response),
    /// Committed, then the held effects run.
    Commit(Response),
    /// Rolled back, storing the token the answer (a preview) carries.
    Answer(Response, Option<PreviewToken>),
}

impl Run {
    fn new(request: [u8; 32], apply: Option<TokenHash>) -> Self {
        Self {
            request,
            apply: apply.map(|token| Applying {
                token,
                queue: Mutex::default(),
                claimed: Mutex::default(),
            }),
            stores: Mutex::default(),
            host: Mutex::default(),
            handed: Mutex::default(),
            viewer: Mutex::default(),
            effects: Effects::default(),
            deferred: Mutex::default(),
        }
    }

    /// Whether this is an apply (a write sent with its preview's token).
    pub(crate) fn is_apply(&self) -> bool {
        self.apply.is_some()
    }

    /// Keeps a store's open transaction, to end once the handler is done.
    pub(crate) fn hold(&self, store: OpenPreview, host: TokenHost) {
        lock(&self.stores).push(store);
        lock(&self.host).get_or_insert(host);
    }

    pub(crate) fn set_viewer(&self, viewer: Viewer) {
        *lock(&self.viewer) = Some(viewer);
    }

    /// The next ID for the command, noted: applying, the preview's, in order, then
    /// `fresh` ones.
    pub(crate) fn next_id(&self, fresh: impl FnOnce() -> Uuid) -> Uuid {
        let queued = self
            .apply
            .as_ref()
            .and_then(|apply| lock(&apply.queue).pop_front());
        let id = queued.unwrap_or_else(fresh);
        lock(&self.handed).push(id);
        id
    }

    /// Lists an email as the write's and holds `send` until it commits.
    pub(crate) fn hold_email(&self, email: PreviewEmail, send: Deferred) {
        lock(&self.effects.emails).push(email);
        self.defer(send);
    }

    /// Holds background work until the write commits.
    pub(crate) fn defer(&self, work: Deferred) {
        lock(&self.deferred).push(work);
    }

    /// Notes the first effect that couldn't run: a preview stops there; an apply too, and
    /// is stale.
    pub(crate) fn block(&self, by: BlockedBy) {
        lock(&self.effects.blocked).get_or_insert(by);
    }

    /// Why a blocked call wasn't sent.
    pub(crate) fn not_sent(&self) -> &'static str {
        match self.apply {
            None => NOT_SENT,
            Some(_) => NOT_PREVIEWED,
        }
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
                lock(&self.effects.calls).push(PreviewCall {
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

    /// Applying, checks the token for the caller `ctx` and marks it used, through `ports`
    /// (in the apply's transaction: a write that rolls back leaves it unused). Unknown,
    /// expired, another caller's or another request's: `preview_token_invalid`; used:
    /// `preview_used`. A preview has nothing to claim.
    pub(crate) async fn claim<P: HasPreviewTokens>(
        &self,
        ports: &P,
        ctx: &Ctx,
        now: Timestamp,
    ) -> Result<(), ApiError> {
        let Some(apply) = &self.apply else {
            return Ok(());
        };
        let hash = apply.token.as_bytes();
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
        *lock(&apply.queue) = token.ids.into();
        *lock(&apply.claimed) = Some(token.digest);
        Ok(())
    }

    /// Ends the stores' transactions and answers for the handler's `response`. A write
    /// that failed answers its failure. A preview answers what the write did. An apply
    /// commits when the write did what its preview did, then runs the effects held until
    /// then; otherwise it rolls back and answers `preview_stale` with what it would do
    /// now. A write that reached an effect its preview couldn't have had is stale too,
    /// whatever it answered.
    async fn finish(&self, response: Response) -> Response {
        let stores = std::mem::take(&mut *lock(&self.stores));
        let recorded = recorded_by(&stores);
        match self.end(response, recorded).await {
            Ok(Ending::Pass(answer)) => {
                let _ = finish_all(stores, false, None).await;
                answer
            }
            Err(e) => {
                let _ = finish_all(stores, false, None).await;
                e.into_response()
            }
            Ok(Ending::Commit(answer)) => match finish_all(stores, true, None).await {
                Ok(()) => {
                    let deferred = std::mem::take(&mut *lock(&self.deferred));
                    for work in deferred {
                        work().await;
                    }
                    answer
                }
                Err(e) => ApiError::internal(format!("committing an apply: {e}")).into_response(),
            },
            Ok(Ending::Answer(answer, token)) => match finish_all(stores, false, token).await {
                Ok(()) => answer,
                Err(e) => ApiError::internal(format!("ending a preview: {e}")).into_response(),
            },
        }
    }

    async fn end(
        &self,
        response: Response,
        recorded: impl Future<Output = Result<WriteRecord, RepoError>>,
    ) -> Result<Ending, ApiError> {
        let blocked = lock(&self.effects.blocked).take();
        if blocked.is_none()
            && (!response.status().is_success() || (self.is_apply() && is_replay(&response)))
        {
            // A replay was applied when the write was made earlier under the same
            // idempotency key: nothing to compare and nothing to keep (the token stays
            // unused).
            return Ok(Ending::Pass(response));
        }
        let claimed = match &self.apply {
            None => None,
            Some(apply) => Some(lock(&apply.claimed).take().ok_or_else(|| {
                ApiError::preview_token_invalid("this route takes no credentials")
            })?),
        };
        let recorded = recorded
            .await
            .map_err(|e| ApiError::internal(format!("reading what a write did: {e}")))?;
        let digest = digest(&recorded)?;
        if blocked.is_none() && claimed.as_ref() == Some(&digest) {
            return Ok(Ending::Commit(response));
        }
        let (parts, body) = response.into_parts();
        let bytes = axum::body::to_bytes(body, MAX_RESPONSE_BYTES)
            .await
            .map_err(|e| ApiError::internal(format!("a previewed response: {e}")))?;
        let viewer = lock(&self.viewer).take();
        let complete = blocked.is_none();
        let mut preview = PreviewBody::build(
            parts.status,
            &bytes,
            &recorded.events,
            viewer.as_ref(),
            std::mem::take(&mut *lock(&self.effects.emails)),
            std::mem::take(&mut *lock(&self.effects.calls)),
            blocked,
        )?;
        let ids = std::mem::take(&mut *lock(&self.handed));
        let host = lock(&self.host).clone();
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
        let mut answer = match self.apply {
            None => (StatusCode::OK, axum::Json(PreviewResponse { preview })).into_response(),
            // What a stale proposal keeps to be approved again.
            Some(_) => ApiError::preview_stale(
                serde_json::to_value(&preview).map_err(ApiError::internal)?,
            )
            .into_response(),
        };
        answer.extensions_mut().insert(PreviewIds(ids));
        answer.extensions_mut().insert(PreviewDigest(digest));
        Ok(Ending::Answer(answer, token))
    }
}

/// Whether `response` replays an earlier write made with the same idempotency key
/// (`extract::idempotent_response`).
fn is_replay(response: &Response) -> bool {
    response
        .headers()
        .get(crate::extract::REPLAYED_HEADER)
        .is_some_and(|v| v == "true")
}

/// The IDs a preview was handed, in order: on the preview's response's extensions, not
/// in its body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewIds(pub Vec<Uuid>);

/// The digest of what a preview did (`digest`), on its response's extensions: what a
/// proposal keeps to apply it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewDigest(pub String);

/// Polls `inner` with `run` current (`current`).
struct Scoped<F> {
    run: Option<Arc<Run>>,
    inner: Pin<Box<F>>,
}

impl<F: Future> Future for Scoped<F> {
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<F::Output> {
        let this = self.get_mut();
        let outer = CURRENT.replace(this.run.clone());
        let polled = this.inner.as_mut().poll(cx);
        CURRENT.set(outer);
        polled
    }
}

/// `inner` with neither a preview nor an apply current: a request sent in process
/// (`internal::dispatch`) is its own, whatever the request sending it is.
pub(crate) fn detached<F: Future>(inner: F) -> impl Future<Output = F::Output> {
    Scoped {
        run: None,
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
        (Some(true), None) => run(request, next, None).await,
        (Some(false), Some(token)) => run(request, next, Some(token)).await,
    }
}

/// Runs `request` as a preview, or (`token`) as an apply.
async fn run(request: Request, next: Next, token: Option<TokenHash>) -> Response {
    let (request, fingerprint) = match fingerprinted(request).await {
        Ok(fingerprinted) => fingerprinted,
        Err(e) => return e.into_response(),
    };
    let run = Arc::new(Run::new(fingerprint, token));
    let response = Scoped {
        run: Some(Arc::clone(&run)),
        inner: Box::pin(next.run(request)),
    }
    .await;
    run.finish(response).await
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
    /// write records the same events over records as they were when previewed (else `409
    /// preview_stale`). Once. Absent when the preview isn't `complete`.
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
        let run = Arc::new(Run::new([0; 32], None));
        let seen = pollster::block_on(Scoped {
            run: Some(Arc::clone(&run)),
            inner: Box::pin(async { previewing() }),
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
    fn an_apply_makes_no_call_its_preview_couldnt_have_made() {
        let apply = Run::new([0; 32], Some(TokenHash::of("hypha_pv_x")));
        assert!(apply.allows_call(&CallPurpose::Extension {
            point: "shipping.rates",
            app: "app_rates".to_string(),
            read_only: true,
        }));
        assert!(!apply.allows_call(&CallPurpose::Extension {
            point: "payments.capture",
            app: "app_pay".to_string(),
            read_only: false,
        }));
        assert!(!apply.allows_call(&CallPurpose::Other));
        assert_eq!(
            *lock(&apply.effects.blocked),
            Some(BlockedBy::ExtensionCall {
                point: "payments.capture",
                app: "app_pay".to_string()
            })
        );
    }

    fn recorded(data: Value) -> WriteRecord {
        WriteRecord {
            events: vec![event(data)],
            rows: Vec::new(),
        }
    }

    #[test]
    fn times_dont_change_a_digest_and_other_values_do() {
        let digest_of = |data| digest(&recorded(data)).unwrap();
        assert_eq!(
            digest_of(serde_json::json!({"due_at": 1, "at": "2026-01-01T00:00:00Z"})),
            digest_of(serde_json::json!({"due_at": 2, "at": "2026-02-01T00:00:00Z"}))
        );
        assert_ne!(
            digest_of(serde_json::json!({"due": 1})),
            digest_of(serde_json::json!({"due": 2}))
        );
    }

    #[test]
    fn rows_digest_alike_in_any_order() {
        let row = |key: &str, version| hypha_core::WrittenRow {
            kind: "inventory_items".to_string(),
            key: key.to_string(),
            change: RowChange::Updated(version),
        };
        let digest_of = |rows| {
            digest(&WriteRecord {
                events: Vec::new(),
                rows,
            })
            .unwrap()
        };
        assert_eq!(
            digest_of(vec![row("a", 1), row("b", 2)]),
            digest_of(vec![row("b", 2), row("a", 1)])
        );
        assert_ne!(
            digest_of(vec![row("a", 1), row("b", 2)]),
            digest_of(vec![row("a", 2), row("b", 1)])
        );
    }

    #[test]
    fn the_first_blocked_effect_is_the_one_reported() {
        let preview = Run::new([0; 32], None);
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
            *lock(&preview.effects.blocked),
            Some(BlockedBy::ExtensionCall {
                point: "payments.refund",
                app: "app_pay".to_string()
            })
        );
        assert_eq!(lock(&preview.effects.calls).len(), 1);
    }
}
