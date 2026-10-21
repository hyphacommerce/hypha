//! Notifications (design note 0028, §7): the store's email settings, its templates (the
//! merchant's or the build's defaults), previews and test sends, and the record of every
//! message with resending.

use axum::{Json, extract::State, http::StatusCode};
use hypha_core::{AssetId, EventId, NotificationId, OrderId, Scope, Stamped};
use hypha_mail::{MailContext, MailEnv, TemplateInput, UpdateSettings};
use hypha_media::{AssetRepo, HasAssets};
use hypha_notifications::{
    Audience, EffectiveTemplate, Notification, NotificationFilter, NotificationSettings,
    NotificationStatus, Rendered, TemplateError, TemplateKey, TemplatePart, TemplateProblem,
    get_notification, get_notification_settings, get_notification_template,
    list_notification_templates, list_notifications, revert_notification_template,
    set_notification_template_enabled,
};
use serde::{Deserialize, Serialize};

use super::{
    page::{ListQuery, PageParams, PageResponse},
    rfc3339, stamp_times,
};
use crate::{
    error::ApiError,
    extract::{Caller, JsonBody, PathId},
    patch,
    redact::{self, Family, Redacted},
    state::{SharedState, Storage},
};

// ---------- shared shapes ----------

/// One of the store's templates. The names are stable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[schema(as = NotificationTemplateKey)]
#[serde(rename_all = "snake_case")]
pub enum TemplateKeyDto {
    /// The frame every other template extends (`{% extends "layout" %}`).
    Layout,
    OrderConfirmation,
    OrderShipped,
    OrderTrackingUpdated,
    OrderCancelled,
    StaffNewOrder,
    StaffInvite,
    StaffPasswordReset,
    CustomerLoginCode,
}

impl From<TemplateKey> for TemplateKeyDto {
    fn from(k: TemplateKey) -> Self {
        match k {
            TemplateKey::Layout => Self::Layout,
            TemplateKey::OrderConfirmation => Self::OrderConfirmation,
            TemplateKey::OrderShipped => Self::OrderShipped,
            TemplateKey::OrderTrackingUpdated => Self::OrderTrackingUpdated,
            TemplateKey::OrderCancelled => Self::OrderCancelled,
            TemplateKey::StaffNewOrder => Self::StaffNewOrder,
            TemplateKey::StaffInvite => Self::StaffInvite,
            TemplateKey::StaffPasswordReset => Self::StaffPasswordReset,
            TemplateKey::CustomerLoginCode => Self::CustomerLoginCode,
        }
    }
}

impl From<TemplateKeyDto> for TemplateKey {
    fn from(k: TemplateKeyDto) -> Self {
        match k {
            TemplateKeyDto::Layout => Self::Layout,
            TemplateKeyDto::OrderConfirmation => Self::OrderConfirmation,
            TemplateKeyDto::OrderShipped => Self::OrderShipped,
            TemplateKeyDto::OrderTrackingUpdated => Self::OrderTrackingUpdated,
            TemplateKeyDto::OrderCancelled => Self::OrderCancelled,
            TemplateKeyDto::StaffNewOrder => Self::StaffNewOrder,
            TemplateKeyDto::StaffInvite => Self::StaffInvite,
            TemplateKeyDto::StaffPasswordReset => Self::StaffPasswordReset,
            TemplateKeyDto::CustomerLoginCode => Self::CustomerLoginCode,
        }
    }
}

/// A template named in the URL; an unknown name is `404 template_unknown`.
fn template_key(text: &str) -> Result<TemplateKey, ApiError> {
    text.parse::<TemplateKey>().map_err(|unknown| {
        ApiError::from(hypha_notifications::CommandError::Template(
            TemplateError::UnknownKey(unknown),
        ))
        .not_found_if("template_unknown")
    })
}

/// Who a template's messages go to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[schema(as = NotificationAudience)]
#[serde(rename_all = "snake_case")]
pub enum AudienceDto {
    /// The order's email address.
    Buyer,
    /// Staff: the settings' recipients, an invitee, someone resetting a password.
    Staff,
}

impl From<Audience> for AudienceDto {
    fn from(a: Audience) -> Self {
        match a {
            Audience::Buyer => Self::Buyer,
            Audience::Staff => Self::Staff,
        }
    }
}

/// Which source of a template a problem is in; `layout` when rendering found it in the
/// shared frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[schema(as = TemplatePart)]
#[serde(rename_all = "snake_case")]
pub enum TemplatePartDto {
    Subject,
    Html,
    Text,
    Layout,
}

impl From<TemplatePart> for TemplatePartDto {
    fn from(p: TemplatePart) -> Self {
        match p {
            TemplatePart::Subject => Self::Subject,
            TemplatePart::Html => Self::Html,
            TemplatePart::Text => Self::Text,
            TemplatePart::Layout => Self::Layout,
        }
    }
}

/// What the template engine reported, as `template_invalid`'s `details` has it too.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct TemplateProblemResponse {
    part: TemplatePartDto,
    /// Counted from 1, when known.
    line: Option<usize>,
    message: String,
}

impl From<&TemplateProblem> for TemplateProblemResponse {
    fn from(p: &TemplateProblem) -> Self {
        Self {
            part: p.part.into(),
            line: p.line,
            message: p.message.clone(),
        }
    }
}

// ---------- settings ----------

#[derive(Serialize, utoipa::ToSchema)]
pub struct NotificationSettingsResponse {
    /// An image from the media library shown at the top of every email.
    logo: Option<AssetId>,
    /// The logo's URL as emails use it; `null` without one or without media storage.
    logo_url: Option<String>,
    /// `#rrggbb`: buttons and links.
    accent: String,
    /// Plain text under every email (the address, legal lines).
    footer: Option<String>,
    /// The name emails come from; `null`: the store's name.
    sender_name: Option<String>,
    reply_to: Option<String>,
    /// Who gets `staff_new_order`.
    staff_recipients: Vec<String>,
    /// The buyer's order page, with `{order}` (and optionally `{token}`, the order's
    /// access token) for links in emails; `null`: emails link to no order.
    order_url: Option<String>,
    /// The operator's from address (`email.from`); `sender_name` goes with it.
    from: String,
    /// The configured sender: `log` (emails are written to the log, not sent; test
    /// sends are refused), `smtp` or `resend`.
    sender: String,
    /// `null` until the settings are first saved (the defaults).
    updated_at: Option<String>,
}

async fn settings_response<S: Storage>(
    state: &SharedState<S>,
    ports: &S::Ports<'_>,
    settings: &NotificationSettings,
) -> Result<NotificationSettingsResponse, ApiError> {
    let logo_url = match settings.logo() {
        Some(logo) => match ports.assets().get(logo).await? {
            Some(asset) => state.mail.asset_url(&asset).await,
            None => None,
        },
        None => None,
    };
    let updated_at = match settings.stamps() {
        Some(stamps) => Some(rfc3339(stamps.updated_at)?),
        None => None,
    };
    Ok(NotificationSettingsResponse {
        logo: settings.logo(),
        logo_url,
        accent: settings.accent().as_str().to_string(),
        footer: settings.footer().map(str::to_string),
        sender_name: settings.sender_name().map(str::to_string),
        reply_to: settings.reply_to().map(ToString::to_string),
        staff_recipients: settings
            .staff_recipients()
            .iter()
            .map(ToString::to_string)
            .collect(),
        order_url: settings.order_url().map(|u| u.as_str().to_string()),
        from: state.mail.sender_address().to_string(),
        sender: state.mail.sender_name().to_string(),
        updated_at,
    })
}

/// The store's email settings: branding, the sender's name, reply-to, staff recipients
/// and the order link; the defaults until first saved.
#[utoipa::path(
    get,
    path = "/admin/notification-settings",
    responses((status = 200, description = "OK", body = NotificationSettingsResponse)),
    extensions(("x-hypha-scopes" = json!(["notifications:read"]))),
)]
pub async fn fetch_settings<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
) -> Result<Json<NotificationSettingsResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let settings = get_notification_settings(&ctx, &ports).await?;
    Ok(Json(settings_response(&state, &ports, &settings).await?))
}

/// JSON Merge Patch: absent keeps, `null` clears (`accent` and `staff_recipients` take
/// no `null`).
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateNotificationSettingsRequest {
    /// An image from the media library: ready and public (`asset_not_ready`,
    /// `asset_not_public`, `asset_kind_not_allowed`). It can't be deleted while it's the
    /// logo.
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<AssetId>, required = false)]
    logo: Option<Option<AssetId>>,
    /// `#rrggbb`.
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = String, required = false)]
    accent: Option<String>,
    /// At most 1000 characters.
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<String>, required = false)]
    footer: Option<Option<String>>,
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<String>, required = false)]
    sender_name: Option<Option<String>>,
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<String>, required = false)]
    reply_to: Option<Option<String>>,
    /// At most 10 addresses; the whole list.
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = Vec<String>, required = false)]
    staff_recipients: Option<Vec<String>>,
    /// An http(s) URL with `{order}`, and optionally `{token}`, e.g.
    /// `https://shop.example.com/order/{order}?token={token}`.
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<String>, required = false)]
    order_url: Option<Option<String>>,
}

/// Changes the email settings.
#[utoipa::path(
    patch,
    path = "/admin/notification-settings",
    request_body = UpdateNotificationSettingsRequest,
    responses((status = 200, description = "OK", body = NotificationSettingsResponse)),
    extensions(("x-hypha-scopes" = json!(["notifications:write"]))),
)]
pub async fn update_settings<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    body: JsonBody<UpdateNotificationSettingsRequest>,
) -> Result<Json<NotificationSettingsResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let input = UpdateSettings {
        logo: req.logo,
        accent: req.accent,
        footer: req.footer,
        sender_name: req.sender_name,
        reply_to: req.reply_to,
        staff_recipients: req.staff_recipients,
        order_url: req.order_url,
    };
    let settings = hypha_mail::update_settings(&ctx, &ports, input).await?;
    Ok(Json(settings_response(&state, &ports, &settings).await?))
}

// ---------- templates ----------

/// A template in the list.
#[derive(Serialize, utoipa::ToSchema)]
pub struct NotificationTemplateSummary {
    key: TemplateKeyDto,
    /// `null` for the layout, which is never sent on its own.
    audience: Option<AudienceDto>,
    /// Whether the sources are the merchant's (else the build's default).
    customized: bool,
    /// Whether its messages are sent. A template that's off records each message as
    /// `skipped`.
    enabled: bool,
    /// The layout, invites and password resets can't be turned off.
    can_disable: bool,
    /// Why the stored sources stopped working (after an upgrade, say): sends use the
    /// default meanwhile.
    problem: Option<TemplateProblemResponse>,
}

impl From<&EffectiveTemplate> for NotificationTemplateSummary {
    fn from(t: &EffectiveTemplate) -> Self {
        Self {
            key: t.key.into(),
            audience: t.key.audience().map(Into::into),
            customized: t.customized,
            enabled: t.enabled,
            can_disable: t.key.can_disable(),
            problem: t.problem.as_ref().map(Into::into),
        }
    }
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct NotificationTemplateList {
    /// Every template: the layout, the buyer's, then the staff's.
    items: Vec<NotificationTemplateSummary>,
}

/// Every template, customized or default.
#[utoipa::path(
    get,
    path = "/admin/notification-templates",
    responses((status = 200, description = "OK", body = NotificationTemplateList)),
    extensions(("x-hypha-scopes" = json!(["notifications:read"]))),
)]
pub async fn list_templates<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
) -> Result<Json<NotificationTemplateList>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let templates = list_notification_templates(&ctx, &ports).await?;
    Ok(Json(NotificationTemplateList {
        items: templates.iter().map(Into::into).collect(),
    }))
}

/// A template with the sources in effect and the variables it can use.
#[derive(Serialize, utoipa::ToSchema)]
pub struct NotificationTemplateResponse {
    #[serde(flatten)]
    summary: NotificationTemplateSummary,
    /// Jinja; empty for the layout. The merchant's when `customized`, else the default.
    subject: String,
    /// Jinja HTML. A message starts with `{% extends "layout" %}` and fills
    /// `{% block preheader %}` and `{% block content %}`.
    html: String,
    /// Jinja plain text; `null`: the text is made from the HTML.
    text: Option<String>,
    /// The template's context, filled with a made-up order and the store's branding:
    /// every variable it can print (money as `{amount, currency, formatted}`, times as
    /// `{iso, formatted}`), for the editor's variable list.
    #[schema(value_type = Object)]
    variables: serde_json::Value,
}

async fn template_response<S: Storage>(
    state: &SharedState<S>,
    ports: &S::Ports<'_>,
    template: &EffectiveTemplate,
) -> Result<NotificationTemplateResponse, ApiError> {
    let branding = hypha_mail::branding(&*state.mail, ports).await?;
    let sample = MailContext::sample(template.key).with_branding(branding.shop, branding.brand);
    Ok(NotificationTemplateResponse {
        summary: template.into(),
        subject: template.sources.subject().to_string(),
        html: template.sources.html().to_string(),
        text: template.sources.text().map(str::to_string),
        variables: serde_json::to_value(&sample).map_err(ApiError::internal)?,
    })
}

/// One template: its sources in effect, whether it's customized and on, and its
/// variables.
#[utoipa::path(
    get,
    path = "/admin/notification-templates/{key}",
    params(("key" = String, Path, pattern = "^[a-z_]+$", example = "order_confirmation")),
    responses((status = 200, description = "OK", body = NotificationTemplateResponse)),
    extensions(("x-hypha-scopes" = json!(["notifications:read"]))),
)]
pub async fn fetch_template<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(key): PathId<String>,
) -> Result<Json<NotificationTemplateResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let key = template_key(&key)?;
    let template = get_notification_template(&ctx, &ports, key).await?;
    Ok(Json(template_response(&state, &ports, &template).await?))
}

/// A template's sources (all of them: this replaces the stored ones).
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SaveNotificationTemplateRequest {
    /// Jinja, one line; required for a message, absent (or empty) for the layout.
    #[serde(default)]
    #[schema(required = false)]
    subject: String,
    html: String,
    /// Absent or `null`: the text is made from the HTML.
    #[serde(default)]
    #[schema(required = false)]
    text: Option<String>,
    /// Absent keeps whether it's on.
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = bool, required = false)]
    enabled: Option<bool>,
}

/// Saves a template. The sources must parse and render against the template's sample
/// in the layout in effect (`template_invalid` with `details: {part, line, message}`);
/// saving the layout renders every message in it. Turning off one that can't be is
/// `template_required`.
#[utoipa::path(
    put,
    path = "/admin/notification-templates/{key}",
    params(("key" = String, Path, pattern = "^[a-z_]+$", example = "order_confirmation")),
    request_body = SaveNotificationTemplateRequest,
    responses((status = 200, description = "OK", body = NotificationTemplateResponse)),
    extensions(("x-hypha-scopes" = json!(["notifications:write"]))),
)]
pub async fn save_template<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(key): PathId<String>,
    body: JsonBody<SaveNotificationTemplateRequest>,
) -> Result<Json<NotificationTemplateResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let key = template_key(&key)?;
    let enabled = match req.enabled {
        Some(enabled) => enabled,
        None => get_notification_template(&ctx, &ports, key).await?.enabled,
    };
    let input = TemplateInput {
        subject: req.subject,
        html: req.html,
        text: req.text,
    };
    let template = hypha_mail::save_template(&ctx, &ports, key, input, enabled).await?;
    Ok(Json(template_response(&state, &ports, &template).await?))
}

/// Whether a template is sent, apart from its sources.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetNotificationTemplateRequest {
    enabled: bool,
}

/// Turns a template on or off, keeping its sources: a default stays a default (and
/// gets later improvements). Turning off one that can't be is `template_required`.
#[utoipa::path(
    patch,
    path = "/admin/notification-templates/{key}",
    params(("key" = String, Path, pattern = "^[a-z_]+$", example = "order_confirmation")),
    request_body = SetNotificationTemplateRequest,
    responses((status = 200, description = "OK", body = NotificationTemplateResponse)),
    extensions(("x-hypha-scopes" = json!(["notifications:write"]))),
)]
pub async fn set_template_enabled<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(key): PathId<String>,
    body: JsonBody<SetNotificationTemplateRequest>,
) -> Result<Json<NotificationTemplateResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let key = template_key(&key)?;
    let template = set_notification_template_enabled(&ctx, &ports, key, req.enabled).await?;
    Ok(Json(template_response(&state, &ports, &template).await?))
}

/// Goes back to the default's sources, keeping whether it's on.
#[utoipa::path(
    delete,
    path = "/admin/notification-templates/{key}",
    params(("key" = String, Path, pattern = "^[a-z_]+$", example = "order_confirmation")),
    responses((status = 200, description = "OK", body = NotificationTemplateResponse)),
    extensions(("x-hypha-scopes" = json!(["notifications:write"]))),
)]
pub async fn revert_template<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(key): PathId<String>,
) -> Result<Json<NotificationTemplateResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let key = template_key(&key)?;
    let template = revert_notification_template(&ctx, &ports, key).await?;
    Ok(Json(template_response(&state, &ports, &template).await?))
}

/// Unsaved sources to render.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PreviewNotificationRequest {
    /// Empty for the layout.
    #[serde(default)]
    #[schema(required = false)]
    subject: String,
    html: String,
    #[serde(default)]
    #[schema(required = false)]
    text: Option<String>,
    /// Render against this order as it is now (needs `orders:read`) rather than the
    /// sample; the shipping templates use its newest fulfilment. Ignored by the staff
    /// account templates.
    #[serde(default)]
    #[schema(required = false)]
    order: Option<OrderId>,
}

impl PreviewNotificationRequest {
    fn into_parts(self) -> (TemplateInput, Option<OrderId>) {
        (
            TemplateInput {
                subject: self.subject,
                html: self.html,
                text: self.text,
            },
            self.order,
        )
    }
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct RenderedNotificationResponse {
    subject: String,
    html: String,
    text: String,
}

impl From<Rendered> for RenderedNotificationResponse {
    fn from(r: Rendered) -> Self {
        Self {
            subject: r.subject,
            html: r.html,
            text: r.text,
        }
    }
}

/// Renders unsaved sources as they'd be sent: against the sample with the store's
/// branding, or a real `order`. The layout previews as an order confirmation in it.
/// `template_invalid` with `details: {part, line, message}` when they don't render.
#[utoipa::path(
    post,
    path = "/admin/notification-templates/{key}/preview",
    params(("key" = String, Path, pattern = "^[a-z_]+$", example = "order_confirmation")),
    request_body = PreviewNotificationRequest,
    responses((status = 200, description = "OK", body = RenderedNotificationResponse)),
    extensions(("x-hypha-scopes" = json!(["notifications:read"]))),
)]
pub async fn preview<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(key): PathId<String>,
    body: JsonBody<PreviewNotificationRequest>,
) -> Result<Json<RenderedNotificationResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let key = template_key(&key)?;
    let (input, order) = req.into_parts();
    // A preview of an order's email shows the buyer's details: only for callers who may
    // see all of them (design note 0029, AP10).
    if order.is_some() {
        redact::require_fields(&ctx, &Scope::CUSTOMER_DATA)?;
    }
    let rendered = hypha_mail::preview(&ctx, &*state.mail, &ports, key, input, order).await?;
    Ok(Json(rendered.into()))
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct TestNotificationResponse {
    /// The calling staff member's address, the only one a test goes to.
    sent_to: String,
}

/// Sends the preview to the calling staff member's own address, its subject marked
/// `[Test]`; not recorded. A staff session only (`staff_only` for API keys); refused
/// with the `log` sender (`email_not_configured`).
#[utoipa::path(
    post,
    path = "/admin/notification-templates/{key}/test",
    params(("key" = String, Path, pattern = "^[a-z_]+$", example = "order_confirmation")),
    request_body = PreviewNotificationRequest,
    responses((status = 200, description = "OK", body = TestNotificationResponse)),
    extensions(("x-hypha-scopes" = json!(["notifications:write"]))),
)]
pub async fn send_test<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(key): PathId<String>,
    body: JsonBody<PreviewNotificationRequest>,
) -> Result<Json<TestNotificationResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let key = template_key(&key)?;
    let (input, order) = req.into_parts();
    let to = hypha_mail::send_test(&ctx, &*state.mail, &ports, key, input, order).await?;
    Ok(Json(TestNotificationResponse {
        sent_to: to.to_string(),
    }))
}

// ---------- messages ----------

/// Where a message is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[schema(as = NotificationStatus)]
#[serde(rename_all = "snake_case")]
pub enum NotificationStatusDto {
    /// Rendered and waiting to be sent, from `next_attempt_at`: new, or tried again after
    /// the provider couldn't be reached (`error` says why).
    Queued,
    /// Being handed to the provider.
    Sending,
    /// The provider accepted it.
    Sent,
    /// The provider refused it, or every attempt failed (`error` says why); final:
    /// resend it to try again.
    Failed,
    /// Not sent on purpose: the template is turned off.
    Skipped,
}

impl From<NotificationStatus> for NotificationStatusDto {
    fn from(s: NotificationStatus) -> Self {
        match s {
            NotificationStatus::Queued => Self::Queued,
            NotificationStatus::Sending => Self::Sending,
            NotificationStatus::Sent => Self::Sent,
            NotificationStatus::Failed => Self::Failed,
            NotificationStatus::Skipped => Self::Skipped,
        }
    }
}

impl From<NotificationStatusDto> for NotificationStatus {
    fn from(s: NotificationStatusDto) -> Self {
        match s {
            NotificationStatusDto::Queued => Self::Queued,
            NotificationStatusDto::Sending => Self::Sending,
            NotificationStatusDto::Sent => Self::Sent,
            NotificationStatusDto::Failed => Self::Failed,
            NotificationStatusDto::Skipped => Self::Skipped,
        }
    }
}

/// One email's record. Bodies aren't returned (they're kept only until it's sent): `POST
/// /admin/notification-templates/{key}/preview` with the `order` shows what it looks
/// like now.
#[derive(Serialize, utoipa::ToSchema)]
pub struct NotificationResponse {
    id: NotificationId,
    template: TemplateKeyDto,
    to: String,
    /// Personal fields left out for this caller (design note 0029, AP10): an API key or
    /// app without the matching `customer_data:*` scope. They are absent from the
    /// object then, whatever the schema says. Never sent to staff or storefronts.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    redacted: Option<Vec<String>>,
    subject: String,
    status: NotificationStatusDto,
    /// Sends tried so far.
    attempts: u32,
    /// The sender that handled it: `log`, `smtp`, `resend`.
    provider: Option<String>,
    /// The provider's ID for it, when it gives one.
    provider_message_id: Option<String>,
    /// Why the last attempt failed, or why it was skipped.
    error: Option<String>,
    /// How it was made, when staff should know: "sent with the default template: …"
    /// when the store's template didn't render on this order.
    note: Option<String>,
    /// The event it answered; `null` for resends and staff account emails.
    event_id: Option<EventId>,
    order_id: Option<OrderId>,
    sent_at: Option<String>,
    /// When a queued message is next tried.
    next_attempt_at: Option<String>,
    created_at: String,
    updated_at: String,
}

impl NotificationResponse {
    pub(crate) fn new(n: &Notification) -> Result<Self, ApiError> {
        let (created_at, updated_at) = stamp_times(n.stamps())?;
        Ok(Self {
            id: n.id(),
            template: n.template().into(),
            to: n.to().to_string(),
            redacted: None,
            subject: n.subject().to_string(),
            status: n.status().into(),
            attempts: n.attempts(),
            provider: n.provider().map(str::to_string),
            provider_message_id: n.provider_message_id().map(str::to_string),
            error: n.error().map(str::to_string),
            note: n.note().map(str::to_string),
            event_id: n.event(),
            order_id: n.order(),
            sent_at: n.sent_at().map(rfc3339).transpose()?,
            next_attempt_at: n.next_attempt_at().map(rfc3339).transpose()?,
            created_at,
            updated_at,
        })
    }
}

#[derive(Deserialize, utoipa::IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct NotificationFilters {
    /// Only this order's emails.
    #[param(value_type = Option<OrderId>)]
    order: Option<OrderId>,
    /// Only messages of this template.
    #[param(inline)]
    template: Option<TemplateKeyDto>,
    /// Only messages in this status.
    #[param(inline)]
    status: Option<NotificationStatusDto>,
}

/// A page of messages (named for the documents).
type NotificationPage = PageResponse<NotificationResponse>;

/// The emails sent, newest first: an order's (`order`), or every one of a template or
/// status. They name buyers and their orders, so `orders:read` is needed too.
#[utoipa::path(
    get,
    path = "/admin/notifications",
    params(PageParams, NotificationFilters),
    responses((status = 200, description = "OK", body = NotificationPage)),
    extensions(("x-hypha-scopes" = json!(["notifications:read", "orders:read"]))),
)]
pub async fn list<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    query: ListQuery<NotificationFilters>,
) -> Result<Json<Redacted>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let (page, filters, scope) = query.parse()?;
    let filter = NotificationFilter {
        order: filters.order,
        template: filters.template.map(Into::into),
        status: filters.status.map(Into::into),
    };
    let page = list_notifications(&ctx, &ports, filter, page).await?;
    let page = PageResponse::new(page, &scope, |n| n.id(), |n| NotificationResponse::new(&n))?;
    Ok(Json(redact::page(&ctx, Family::Notification, page)?))
}

/// One email's record (`orders:read` too, as the list).
#[utoipa::path(
    get,
    path = "/admin/notifications/{id}",
    params(("id" = NotificationId, Path)),
    responses((status = 200, description = "OK", body = NotificationResponse)),
    extensions(("x-hypha-scopes" = json!(["notifications:read", "orders:read"]))),
)]
pub async fn fetch<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<NotificationId>,
) -> Result<Json<Redacted>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let notification = get_notification(&ctx, &ports, id).await?;
    Ok(Json(redact::one(
        &ctx,
        Family::Notification,
        NotificationResponse::new(&notification)?,
    )?))
}

/// Sends an order's email again ("the customer didn't get it"), rendered from the order
/// as it is now with the templates in effect, even if the template is off: a new record
/// (201), `queued` for the send job, which goes at once (read the record again for how
/// it went). Invites and resets can't be resent (`notification_not_resendable`: their
/// token isn't kept).
#[utoipa::path(
    post,
    path = "/admin/notifications/{id}/resend",
    params(("id" = NotificationId, Path)),
    responses((status = 201, description = "Created", body = NotificationResponse)),
    extensions(("x-hypha-scopes" = json!(["notifications:write"]))),
)]
pub async fn resend<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<NotificationId>,
) -> Result<(StatusCode, Json<Redacted>), ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let queued = hypha_mail::resend(&ctx, &*state.mail, &ports, id).await?;
    Ok((
        StatusCode::CREATED,
        Json(redact::one(
            &ctx,
            Family::Notification,
            NotificationResponse::new(&queued)?,
        )?),
    ))
}
