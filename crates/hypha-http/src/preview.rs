//! Previewing an admin write: `Hypha-Preview: 1` on any non-GET request under `/admin/`.
//! The request runs for real (the same handler, command and ports) and answers what it
//! would have done instead of doing it:
//!
//! - the ports it gets (`AppState::public_ports`, so `Caller::authenticate` too) run in a
//!   preview (`Storage::begin_preview`): one transaction, rolled back once the handler is
//!   done, after the events its writes recorded are read back;
//! - the host's capabilities (`host.rs`, the one door to effects outside the database)
//!   look for the request's `Preview` while they're called: emails are recorded, not sent;
//!   calls to apps are blocked unless the extension point is read-only; media storage is
//!   blocked; background work and wake-ups are dropped; the IDs handed out are noted.
//!
//! The request's `Preview` is current while its future is polled (`InPreview`), so a
//! capability called from anywhere in the handler finds it without being passed it: a
//! poll runs one request's code at a time on its thread, natively and on Workers alike.
//!
//! A write that fails answers its failure as it would have without the header; one that
//! reached a blocked effect answers `200` with `complete: false` and what blocked it.

use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard};
use std::task::{Context, Poll};

use axum::{
    body::Body,
    extract::Request,
    http::{Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use hypha_core::{Ctx, RepoError, StoredEvent};
use hypha_custom_fields::{CallerKind, CustomFieldDefinition, FieldCaller};
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use crate::error::ApiError;
use crate::host::BoxFuture;
use crate::outbound::CallPurpose;
use crate::redact::Redaction;

/// The request header that asks for a preview: `1`. `0` (or none) runs the write.
pub const PREVIEW_HEADER: &str = "hypha-preview";

/// The most of a handler's answer a preview reads (its `response.body`).
const MAX_RESPONSE_BYTES: usize = 16 << 20;

thread_local! {
    /// The preview whose request is being polled (`InPreview`).
    static CURRENT: RefCell<Option<Arc<Preview>>> = const { RefCell::new(None) };
}

/// The preview of the request being polled, if it is one.
pub(crate) fn current() -> Option<Arc<Preview>> {
    CURRENT.with_borrow(|current| current.clone())
}

/// A store's open preview (`Storage::begin_preview`): what ends it, giving back the events
/// its writes recorded. Boxed, so this module knows no storage; built by each host for its
/// concrete storage, where the future is provably `Send`.
pub struct OpenPreview(Box<EndPreview>);

type EndPreview = dyn FnOnce() -> BoxFuture<Result<Vec<StoredEvent>, RepoError>> + Send;

impl OpenPreview {
    pub fn new(
        end: impl FnOnce() -> BoxFuture<Result<Vec<StoredEvent>, RepoError>> + Send + 'static,
    ) -> Self {
        Self(Box::new(end))
    }

    async fn end(self) -> Result<Vec<StoredEvent>, RepoError> {
        (self.0)().await
    }
}

/// One preview request's record: its stores' open previews, what its effects would have
/// been, the first one it couldn't preview, the IDs it was handed, and who it is, for
/// redacting its events.
#[derive(Default)]
pub struct Preview {
    stores: Mutex<Vec<OpenPreview>>,
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
    /// Keeps a store's open preview, to end once the handler is done.
    pub(crate) fn hold(&self, store: OpenPreview) {
        lock(&self.stores).push(store);
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
        let mut events = Vec::new();
        let mut failed = None;
        // Every store's preview is ended, whatever happened to the one before.
        for store in stores {
            match store.end().await {
                Ok(mut written) => events.append(&mut written),
                Err(e) => failed = Some(e),
            }
        }
        let (parts, body) = response.into_parts();
        let bytes = match axum::body::to_bytes(body, MAX_RESPONSE_BYTES).await {
            Ok(bytes) => bytes,
            Err(e) => {
                return ApiError::internal(format!("a previewed response: {e}")).into_response();
            }
        };
        let blocked = lock(&self.blocked).take();
        if blocked.is_none() && !parts.status.is_success() {
            return Response::from_parts(parts, Body::from(bytes));
        }
        if let Some(e) = failed {
            return ApiError::internal(format!("ending a preview: {e}")).into_response();
        }
        let viewer = lock(&self.viewer).take();
        let events = match events
            .iter()
            .map(|event| PreviewEvent::new(event, viewer.as_ref()))
            .collect::<Result<Vec<_>, _>>()
        {
            Ok(events) => events,
            Err(e) => return e.into_response(),
        };
        let removes_email = viewer
            .as_ref()
            .is_none_or(|v| Redaction::of(&v.ctx).removes_email());
        let emails = std::mem::take(&mut *lock(&self.emails))
            .into_iter()
            .map(|email| email.redacted(removes_email))
            .collect();
        let response = match blocked {
            // What a write answers after a blocked effect is that effect's failure, not
            // what it would have answered.
            Some(_) if !parts.status.is_success() => None,
            _ => Some(PreviewedResponse {
                status: parts.status.as_u16(),
                body: match bytes.is_empty() {
                    true => Value::Null,
                    false => serde_json::from_slice(&bytes).unwrap_or(Value::Null),
                },
            }),
        };
        let preview = PreviewBody {
            complete: blocked.is_none(),
            response,
            events,
            emails,
            calls: std::mem::take(&mut *lock(&self.calls)),
            blocked_by: blocked,
        };
        let ids = PreviewIds(std::mem::take(&mut *lock(&self.ids)));
        let mut answer = (StatusCode::OK, axum::Json(PreviewResponse { preview })).into_response();
        answer.extensions_mut().insert(ids);
        answer
    }
}

/// The IDs a preview was handed, in order: on the preview's response's extensions, not
/// in its body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewIds(pub Vec<Uuid>);

/// Polls `inner` with `preview` current (`current`).
struct InPreview<F> {
    preview: Arc<Preview>,
    inner: Pin<Box<F>>,
}

impl<F: Future> Future for InPreview<F> {
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<F::Output> {
        let this = self.get_mut();
        let outer = CURRENT.replace(Some(Arc::clone(&this.preview)));
        let polled = this.inner.as_mut().poll(cx);
        CURRENT.set(outer);
        polled
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

/// Whether a request is an admin write: any method but `GET`, `HEAD` and `OPTIONS` under
/// `/admin/`.
fn is_admin_write(request: &Request) -> bool {
    !matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    ) && request.uri().path().starts_with("/admin/")
}

/// The middleware that previews admin writes sent with `Hypha-Preview: 1`; reads ignore the
/// header. Hosts layer it over every route, their spores' included (`with_previews`).
pub async fn middleware(request: Request, next: Next) -> Response {
    if !is_admin_write(&request) {
        return next.run(request).await;
    }
    match asks_for_preview(&request) {
        Some(false) => next.run(request).await,
        None => ApiError::invalid_preview_header().into_response(),
        Some(true) => {
            let preview = Arc::new(Preview::default());
            let response = InPreview {
                preview: Arc::clone(&preview),
                inner: Box::pin(next.run(request)),
            }
            .await;
            preview.finish(response).await
        }
    }
}

/// `router` with admin writes previewable (`middleware`).
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
        let seen = pollster::block_on(InPreview {
            preview: Arc::clone(&preview),
            inner: Box::pin(async { current().is_some() }),
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
