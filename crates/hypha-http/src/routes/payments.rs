//! Payment methods (design note 0027, §7): how shoppers can pay, as settings; the
//! payments shown on orders; and what happens to an app's payments (design note 0029,
//! §3.1): the app reporting results, staff asking it to capture, void or refund.

use axum::{Json, extract::State, http::StatusCode, response::Response};
use hypha_core::{ChannelId, MarketId, Money, OrderId, PaymentId, PaymentMethodId, Stamped};
use hypha_order_actions::{PaymentEvent, PaymentEventKind, record_payment_event};
use hypha_payments::{
    ActOnPayment, CreatePaymentMethod, NextAction, Payment, PaymentAction, PaymentMethod,
    PaymentStatus, UpdatePaymentMethod, act_on_payment, create_payment_method,
    delete_payment_method, get_payment_method, list_payment_methods, update_payment_method,
};
use serde::{Deserialize, Serialize};

use super::{orders::FinancialStatusDto, rfc3339, stamp_times};
use crate::{
    error::ApiError,
    extract::{Caller, JsonBody, KeyedJson, PathId, idempotent_response},
    patch,
    state::{SharedState, Storage},
};

#[derive(Serialize, utoipa::ToSchema)]
pub struct PaymentMethodResponse {
    id: PaymentMethodId,
    /// `manual` (cash on delivery, bank transfer: staff record the money), or the key of
    /// the app that collects (`app_<name>`).
    provider: String,
    /// What shoppers see.
    name: String,
    /// Shown to the shopper after checkout and on the order (bank details, "pay the
    /// courier"); `null` for none.
    instructions: Option<String>,
    /// The markets it's offered in; empty for every market.
    markets: Vec<MarketId>,
    /// The sales channels it's offered in; empty for every channel.
    channels: Vec<ChannelId>,
    enabled: bool,
    /// Shoppers see methods by position, then by ID.
    position: u32,
    /// The provider's own settings (`{}` for `manual` and apps, which keep their own).
    #[schema(value_type = Object)]
    settings: serde_json::Value,
    /// When it was created, RFC 3339 in UTC.
    created_at: String,
    /// When it last changed, RFC 3339 in UTC.
    updated_at: String,
}

impl PaymentMethodResponse {
    fn new(m: &PaymentMethod) -> Result<Self, ApiError> {
        let (created_at, updated_at) = stamp_times(m.stamps())?;
        Ok(Self {
            id: m.id(),
            provider: m.provider().to_string(),
            name: m.name().to_string(),
            instructions: m.instructions().map(str::to_string),
            markets: m.markets().to_vec(),
            channels: m.channels().to_vec(),
            enabled: m.enabled(),
            position: m.position(),
            settings: m.settings().clone(),
            created_at,
            updated_at,
        })
    }
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct PaymentMethodListResponse {
    /// By position, then ID. A store has a handful, so they come in one list.
    items: Vec<PaymentMethodResponse>,
}

/// Every payment method, in the order shoppers see them.
#[utoipa::path(
    get,
    path = "/admin/payment-methods",
    responses((status = 200, description = "OK", body = PaymentMethodListResponse)),
    extensions(("x-hypha-scopes" = json!(["payments:read"]))),
)]
pub async fn list<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
) -> Result<Json<PaymentMethodListResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let methods = list_payment_methods(&ctx, &ports).await?;
    Ok(Json(PaymentMethodListResponse {
        items: methods
            .iter()
            .map(PaymentMethodResponse::new)
            .collect::<Result<_, _>>()?,
    }))
}

/// Who a payment provider is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[schema(as = PaymentProviderKind)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKindDto {
    /// Built in: staff record the money (`manual`).
    Manual,
    /// A spore built into the API (design note 0030).
    Spore,
    /// An installed app answering `payments.start` (design note 0029, §3.1).
    App,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct PaymentProviderResponse {
    /// What a payment method names as its `provider`.
    key: String,
    /// The app's or spore's name; "Manual" for `manual`.
    name: String,
    kind: ProviderKindDto,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct PaymentProviderListResponse {
    /// `manual` first, then the spores' providers, then the apps', by key.
    items: Vec<PaymentProviderResponse>,
}

/// The providers a payment method may name now: `manual`, each enabled spore's (design
/// note 0030), and each installed, enabled app answering `payments.start`. What an
/// admin offers when creating a method.
#[utoipa::path(
    get,
    path = "/admin/payment-providers",
    responses((status = 200, description = "OK", body = PaymentProviderListResponse)),
    extensions(("x-hypha-scopes" = json!(["payments:read"]))),
)]
pub async fn list_providers<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
) -> Result<Json<PaymentProviderListResponse>, ApiError> {
    use hypha_apps::{AppRepo, HasApps};
    use hypha_core::AppKind;
    use hypha_extensions::ExtensionPoint;
    let (ports, ctx) = caller.authenticate(&state).await?;
    ctx.require(hypha_core::Scope::PaymentsRead)
        .map_err(hypha_payments::CommandError::from)?;
    // Every app at once: spores and apps alike are read from it.
    let mut listed = ports.apps().list().await.map_err(ApiError::internal)?;
    listed.sort_by(|a, b| a.key().cmp(b.key()));
    let mut items = Vec::new();
    let mut spores = Vec::new();
    for key in hypha_payments::PaymentProviders::keys(state.storage.payment_providers()) {
        if key.is_manual() {
            items.push(PaymentProviderResponse {
                key: key.to_string(),
                name: "Manual".to_string(),
                kind: ProviderKindDto::Manual,
            });
            continue;
        }
        if let Some(spore) = hypha_apps::spore_in(&listed, key.as_str()).filter(|a| a.enabled()) {
            spores.push(PaymentProviderResponse {
                key: key.to_string(),
                name: spore.name().to_string(),
                kind: ProviderKindDto::Spore,
            });
        }
    }
    spores.sort_by(|a, b| a.key.cmp(&b.key));
    items.extend(spores);
    let apps = listed
        .iter()
        .filter(|app| app.kind() == AppKind::App && app.enabled());
    let point = crate::app_endpoints::known(hypha_extensions::PaymentsStart::NAME)
        .ok_or_else(|| ApiError::internal("payments.start isn't a known point"))?;
    // From the listing: an app installed from its manifest needs no further read.
    for app in apps {
        let answers = hypha_apps::endpoint_of(&ports, app, point)
            .await
            .map_err(ApiError::internal)?;
        if answers.is_some() {
            items.push(PaymentProviderResponse {
                key: app.key().to_string(),
                name: app.name().to_string(),
                kind: ProviderKindDto::App,
            });
        }
    }
    Ok(Json(PaymentProviderListResponse { items }))
}

fn default_true() -> bool {
    true
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreatePaymentMethodRequest {
    /// `manual`, or an app's key `app_<name>`: an installed, enabled app answering the
    /// extension point `payments.start` (`payment_provider_unknown` otherwise).
    provider: String,
    /// Up to 255 characters, e.g. "Cash on delivery".
    name: String,
    /// Up to 2000 characters, shown after checkout; blank is none.
    #[serde(default)]
    instructions: Option<String>,
    /// Markets it's offered in (absent or empty: every one). Not checked: an unknown
    /// one never matches.
    #[serde(default)]
    markets: Vec<MarketId>,
    /// Sales channels it's offered in (absent or empty: every one); unchecked likewise.
    #[serde(default)]
    channels: Vec<ChannelId>,
    /// `true` if absent.
    #[serde(default = "default_true")]
    enabled: bool,
    /// Where shoppers see it; absent puts it after every method there is.
    #[serde(default)]
    position: Option<u32>,
    /// The provider's settings, checked by it (`invalid_payment_settings`); `manual` and
    /// apps take none (`{}`, the default).
    #[serde(default)]
    #[schema(value_type = Option<Object>)]
    settings: Option<serde_json::Value>,
}

/// Creates a payment method.
#[utoipa::path(
    post,
    path = "/admin/payment-methods",
    request_body = CreatePaymentMethodRequest,
    responses((status = 201, description = "Created", body = PaymentMethodResponse)),
    extensions(("x-hypha-scopes" = json!(["payments:write"]))),
)]
pub async fn create<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    body: JsonBody<CreatePaymentMethodRequest>,
) -> Result<(StatusCode, Json<PaymentMethodResponse>), ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let input = CreatePaymentMethod {
        provider: req.provider,
        name: req.name,
        instructions: req.instructions,
        markets: req.markets,
        channels: req.channels,
        enabled: req.enabled,
        position: req.position,
        settings: req.settings,
    };
    let providers = state.payments(&ports);
    let method = create_payment_method(&ctx, &ports, &state.ids, &providers, input).await?;
    Ok((
        StatusCode::CREATED,
        Json(PaymentMethodResponse::new(&method)?),
    ))
}

/// A payment method.
#[utoipa::path(
    get,
    path = "/admin/payment-methods/{id}",
    params(("id" = PaymentMethodId, Path)),
    responses((status = 200, description = "OK", body = PaymentMethodResponse)),
    extensions(("x-hypha-scopes" = json!(["payments:read"]))),
)]
pub async fn fetch<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<PaymentMethodId>,
) -> Result<Json<PaymentMethodResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let method = get_payment_method(&ctx, &ports, id)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("payment_method_not_found"))?;
    Ok(Json(PaymentMethodResponse::new(&method)?))
}

/// A JSON Merge Patch of a payment method. The provider can't change.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdatePaymentMethodRequest {
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = String, required = false)]
    name: Option<String>,
    /// `null` clears them.
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<String>, required = false)]
    instructions: Option<Option<String>>,
    /// Replaces the list; empty for every market.
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = Vec<MarketId>, required = false)]
    markets: Option<Vec<MarketId>>,
    /// Replaces the list; empty for every channel.
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = Vec<ChannelId>, required = false)]
    channels: Option<Vec<ChannelId>>,
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = bool, required = false)]
    enabled: Option<bool>,
    /// Where shoppers see it: reordering is setting positions.
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = u32, required = false)]
    position: Option<u32>,
    /// Replaces the provider's settings, checked by it.
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = Object, required = false)]
    settings: Option<serde_json::Value>,
}

/// Changes a payment method. Payments already made keep the name and instructions they
/// copied.
#[utoipa::path(
    patch,
    path = "/admin/payment-methods/{id}",
    params(("id" = PaymentMethodId, Path)),
    request_body = UpdatePaymentMethodRequest,
    responses((status = 200, description = "OK", body = PaymentMethodResponse)),
    extensions(("x-hypha-scopes" = json!(["payments:write"]))),
)]
pub async fn update<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<PaymentMethodId>,
    body: JsonBody<UpdatePaymentMethodRequest>,
) -> Result<Json<PaymentMethodResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let input = UpdatePaymentMethod {
        name: req.name,
        instructions: req.instructions,
        markets: req.markets,
        channels: req.channels,
        enabled: req.enabled,
        position: req.position,
        settings: req.settings,
    };
    let providers = state.payments(&ports);
    let method = update_payment_method(&ctx, &ports, &providers, id, input)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("payment_method_not_found"))?;
    Ok(Json(PaymentMethodResponse::new(&method)?))
}

/// Deletes a payment method. One an open (pending or authorized) payment uses is
/// refused (`payment_method_in_use`, 409): disable it instead. Payments it made keep
/// its name.
#[utoipa::path(
    delete,
    path = "/admin/payment-methods/{id}",
    params(("id" = PaymentMethodId, Path)),
    responses((status = 204, description = "No content")),
    extensions(("x-hypha-scopes" = json!(["payments:write"]))),
)]
pub async fn delete<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<PaymentMethodId>,
) -> Result<StatusCode, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    delete_payment_method(&ctx, &ports, id)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("payment_method_not_found"))?;
    Ok(StatusCode::NO_CONTENT)
}

/// `PaymentStatus` on the wire (design note 0027, PA7).
#[derive(Clone, Copy, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PaymentStatusDto {
    Pending,
    Authorized,
    Paid,
    Failed,
    Voided,
    PartiallyRefunded,
    Refunded,
}

impl From<PaymentStatus> for PaymentStatusDto {
    fn from(s: PaymentStatus) -> Self {
        match s {
            PaymentStatus::Pending => PaymentStatusDto::Pending,
            PaymentStatus::Authorized => PaymentStatusDto::Authorized,
            PaymentStatus::Paid => PaymentStatusDto::Paid,
            PaymentStatus::Failed => PaymentStatusDto::Failed,
            PaymentStatus::Voided => PaymentStatusDto::Voided,
            PaymentStatus::PartiallyRefunded => PaymentStatusDto::PartiallyRefunded,
            PaymentStatus::Refunded => PaymentStatusDto::Refunded,
        }
    }
}

/// A payment as the admin sees it on an order, and alone (`GET /admin/payments/{id}`).
#[derive(Serialize, utoipa::ToSchema)]
pub struct PaymentResponse {
    id: PaymentId,
    /// The order it collects for.
    order: OrderId,
    /// The method it was made with; `null` for money staff recorded on an order placed
    /// without one.
    method: Option<PaymentMethodId>,
    /// The method's name when the payment was made.
    method_name: Option<String>,
    /// `manual`, or the key of the app that collects it.
    provider: String,
    amount: Money,
    status: PaymentStatusDto,
    /// What was given back.
    refunded: Money,
    /// The provider's ID, or for manual payments what staff typed (a bank reference).
    reference: Option<String>,
    /// The provider's attempt the payment collects through (a Stripe PaymentIntent), set by
    /// its start or a report naming it, for a provider whose IDs name attempts: what
    /// reports naming another are refused for (`payment_reference_mismatch`). Unlike
    /// `reference`, staff's text or a capture's own ID never replace it. Null: none named
    /// yet (a paid or authorized report naming one is then held to `reference`).
    attempt: Option<String>,
    /// The method's instructions at checkout, as the shopper was shown them.
    instructions: Option<String>,
    /// When the money was collected, RFC 3339 in UTC.
    paid_at: Option<String>,
    /// When it was voided, RFC 3339 in UTC.
    voided_at: Option<String>,
    /// When it was created, RFC 3339 in UTC.
    created_at: String,
    /// When it last changed, RFC 3339 in UTC.
    updated_at: String,
}

impl PaymentResponse {
    pub(crate) fn new(p: &Payment) -> Result<Self, ApiError> {
        let (created_at, updated_at) = stamp_times(p.stamps())?;
        Ok(Self {
            id: p.id(),
            order: p.order(),
            method: p.method(),
            method_name: p.method_name().map(str::to_string),
            provider: p.provider().to_string(),
            amount: p.amount(),
            status: p.status().into(),
            refunded: p.refunded(),
            reference: p.reference().map(str::to_string),
            attempt: p.attempt().map(str::to_string),
            instructions: p.instructions().map(str::to_string),
            paid_at: p.paid_at().map(rfc3339).transpose()?,
            voided_at: p.voided_at().map(rfc3339).transpose()?,
            created_at,
            updated_at,
        })
    }

    pub(crate) fn all(payments: &[Payment]) -> Result<Vec<Self>, ApiError> {
        payments.iter().map(Self::new).collect()
    }
}

/// What a storefront shows of an order's payment: how the shopper chose to pay, where
/// it is, and what they were told to do.
#[derive(Serialize, utoipa::ToSchema)]
pub struct PlacedPaymentResponse {
    status: PaymentStatusDto,
    /// The method's name when the order was placed; `null` for a payment staff
    /// recorded on an order placed without one.
    method_name: Option<String>,
    /// What the shopper was told at checkout (bank details); `null` for none.
    instructions: Option<String>,
    amount: Money,
}

impl PlacedPaymentResponse {
    pub(crate) fn new(p: &Payment) -> Self {
        Self {
            status: p.status().into(),
            method_name: p.method_name().map(str::to_string),
            instructions: p.instructions().map(str::to_string),
            amount: p.amount(),
        }
    }
}

/// What the shopper must do next to pay (design note 0027, §3; 0029, §7).
#[derive(Serialize, utoipa::ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NextActionResponse {
    /// Nothing online: show the instructions (cash on delivery, bank transfer). Empty
    /// when the method has none.
    Instructions { instructions: String },
    /// Mount the payment app's storefront element (a custom element, loaded from
    /// `GET /storefront/app-extensions?target=checkout.payment`) with these properties
    /// (a card form's client secret), and wait for its `hypha:payment-confirmed` event.
    ClientWidget {
        element: String,
        #[schema(value_type = Object)]
        props: serde_json::Map<String, serde_json::Value>,
    },
    /// Send the shopper to the provider's page.
    Redirect { url: String },
}

impl From<&NextAction> for NextActionResponse {
    fn from(a: &NextAction) -> Self {
        match a {
            NextAction::Instructions(instructions) => NextActionResponse::Instructions {
                instructions: instructions.clone(),
            },
            NextAction::ClientWidget { element, props } => NextActionResponse::ClientWidget {
                element: element.clone(),
                props: props.clone(),
            },
            NextAction::Redirect { url } => NextActionResponse::Redirect { url: url.clone() },
        }
    }
}

/// A payment method as a storefront lists it for a cart.
#[derive(Serialize, utoipa::ToSchema)]
pub struct CartPaymentMethodResponse {
    id: PaymentMethodId,
    name: String,
    /// `manual` or an app's key: what the storefront shows after choosing (instructions,
    /// or the app's widget).
    provider: String,
    /// Shown with the choice and after checkout; `null` for none.
    instructions: Option<String>,
}

impl CartPaymentMethodResponse {
    pub(crate) fn new(m: &PaymentMethod) -> Self {
        Self {
            id: m.id(),
            name: m.name().to_string(),
            provider: m.provider().to_string(),
            instructions: m.instructions().map(str::to_string),
        }
    }
}

// ---------- an app's payments (design note 0029, §3.1) ----------

/// What a payment's provider reports (`type`): the status the payment is in now.
#[derive(Clone, Copy, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PaymentEventTypeDto {
    Authorized,
    Paid,
    Failed,
    Voided,
    PartiallyRefunded,
    Refunded,
}

impl From<PaymentEventTypeDto> for PaymentEventKind {
    fn from(t: PaymentEventTypeDto) -> Self {
        match t {
            PaymentEventTypeDto::Authorized => PaymentEventKind::Authorized,
            PaymentEventTypeDto::Paid => PaymentEventKind::Paid,
            PaymentEventTypeDto::Failed => PaymentEventKind::Failed,
            PaymentEventTypeDto::Voided => PaymentEventKind::Voided,
            PaymentEventTypeDto::PartiallyRefunded => PaymentEventKind::PartiallyRefunded,
            PaymentEventTypeDto::Refunded => PaymentEventKind::Refunded,
        }
    }
}

/// An RFC 3339 instant in a body.
fn instant<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Option<hypha_core::Timestamp>, D::Error> {
    let text: Option<String> = Option::deserialize(d)?;
    text.map(|t| {
        t.parse::<jiff::Timestamp>()
            .map(|t| hypha_core::Timestamp::from_unix_micros(t.as_microsecond()))
            .map_err(|e| serde::de::Error::custom(format!("not an RFC 3339 instant: {e}")))
    })
    .transpose()
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PaymentEventRequest {
    #[serde(rename = "type")]
    kind: PaymentEventTypeDto,
    /// In the payment's minor units. For a refund, how much this one gave back
    /// (required for `partially_refunded`; for `refunded`, absent is all that was left).
    /// Otherwise absent or the payment's whole amount (`payment_amount_mismatch`).
    #[serde(default)]
    amount: Option<i64>,
    /// The provider's ID for the payment (a PaymentIntent's `pi_…`), kept on it. For a
    /// refund (`partially_refunded`, `refunded`), the provider's ID for that refund
    /// (Stripe's `re_…`), kept in the payment's refund list, not as its reference: a
    /// refund reported again under an ID counted already changes nothing. An app must
    /// send it for refunds (`422 refund_reference_required`). Up to 255 characters.
    #[serde(default)]
    reference: Option<String>,
    /// The provider's ID for the attempt this report is about, for a provider whose IDs
    /// name attempts (a Stripe PaymentIntent's `pi_…`; for a refund, the intent it
    /// refunds). A paid or authorized report keeps it as the payment's attempt (as does
    /// the start, for a provider answering one); Hypha then collects the payment once: a
    /// report naming another attempt is `409 payment_reference_mismatch` (`details:
    /// {status, held, reported}`) unless it pays or authorizes a pending payment (started
    /// again); give back what that attempt took. On a payment past pending with no
    /// attempt yet (`attempt` null on it: its reports named none, or it's from before
    /// attempts were kept), a paid or authorized report naming one is held to the
    /// payment's `reference` instead, so name attempts from the first report; its
    /// declines, voids and refunds aren't checked. Absent, nothing is checked (a capture
    /// or void carrying an ID of its own leaves it out). Up to 255 characters.
    #[serde(default)]
    attempt: Option<String>,
    /// When it happened at the provider, RFC 3339; absent is now.
    #[serde(default, deserialize_with = "instant")]
    #[schema(value_type = Option<String>)]
    occurred_at: Option<hypha_core::Timestamp>,
}

/// A payment after a report, and the payment status of its order.
#[derive(Serialize, utoipa::ToSchema)]
pub struct PaymentEventResponse {
    payment: PaymentResponse,
    order: OrderId,
    /// The order's payment status, derived from its payments.
    financial_status: FinancialStatusDto,
}

/// What a payment's provider reports (design note 0029, §3.1): a payment app tells
/// Hypha what its provider did (Stripe's webhook landed on the app). Moves the payment
/// and its order's payment status in one batch, recording `payment.authorized`,
/// `.paid`, `.failed`, `.voided` or `.refunded` (and `order.paid`). Reporting the
/// status a payment is in already changes nothing. Send an `Idempotency-Key` (the
/// provider's event ID): a retry with it answers as the first did. A refund is counted
/// once per `reference` (the provider's refund ID), whatever the key. A move the payment's status doesn't allow is `409
/// invalid_payment_transition`; a refund that doesn't fit, `422 invalid_refund_amount`.
#[utoipa::path(
    post,
    path = "/admin/payments/{id}/events",
    params(("id" = PaymentId, Path), ("Idempotency-Key" = Option<String>, Header, description = "Makes the request safe to retry: a retry with the same key and body replays the first answer (marked `Idempotent-Replayed: true`) instead of repeating the effect. Same key with another body: `422 idempotency_key_reused`.")),
    request_body = PaymentEventRequest,
    responses((status = 200, description = "OK", body = PaymentEventResponse, headers(("Idempotent-Replayed" = String, description = "`true` when this answer replays an earlier request with the same `Idempotency-Key`.")))),
    extensions(("x-hypha-scopes" = json!(["payments:write"]))),
)]
pub async fn report_event<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<PaymentId>,
    req: KeyedJson<PaymentEventRequest>,
) -> Result<Response, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = req.parse()?;
    let input = PaymentEvent {
        payment: id,
        kind: req.body.kind.into(),
        amount: req.body.amount,
        reference: req.body.reference,
        attempt: req.body.attempt,
        occurred_at: req.body.occurred_at,
        idempotency: req.idempotency,
    };
    let done = record_payment_event(&ctx, &ports, &state.clock, input).await?;
    let payment = done
        .value
        .payments
        .iter()
        .find(|p| p.id() == id)
        .ok_or_else(|| ApiError::internal(format!("payment {id} left its order")))?;
    Ok(idempotent_response(
        StatusCode::OK,
        PaymentEventResponse {
            payment: PaymentResponse::new(payment)?,
            order: done.value.order.id(),
            financial_status: done.value.order.financial_status().into(),
        },
        done.replayed,
    ))
}

/// One payment: where it stands, what was collected and given back, the provider's
/// reference. A payment app reads the payment it was asked to start or act on;
/// reading an order's payments is `orders:read`, like the order.
#[utoipa::path(
    get,
    path = "/admin/payments/{id}",
    params(("id" = PaymentId, Path)),
    responses((status = 200, description = "OK", body = PaymentResponse)),
    extensions(("x-hypha-scopes" = json!(["orders:read"]))),
)]
pub async fn fetch_payment<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<PaymentId>,
) -> Result<Json<PaymentResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let payment = hypha_payments::get_payment(&ctx, &ports, id).await?;
    Ok(Json(PaymentResponse::new(&payment)?))
}

/// A capture or a void: of the whole payment, so the body is `{}`.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct WholePaymentRequest {}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RefundRequest {
    /// In the payment's minor units, more than zero and at most what's left to give back;
    /// absent for all of it.
    #[serde(default)]
    amount: Option<i64>,
}

/// The provider took the action. Unless it said it's done already (`completed`), nothing
/// has moved yet: the payment changes when the provider reports what happened
/// (`POST /admin/payments/{id}/events`).
#[derive(Serialize, utoipa::ToSchema)]
pub struct PaymentActionAcceptedResponse {
    /// The provider's ID for the capture or refund, when it gave one.
    reference: Option<String>,
    /// Whether the action is done and recorded: a provider built into the API (a spore)
    /// may answer that at once, and `payment` has moved. `false`: the payment moves when
    /// the provider reports.
    completed: bool,
    /// The payment as it is now.
    payment: PaymentResponse,
}

/// Asks the payment's app to `action` it; the payment moves when the app reports.
async fn act<S: Storage>(
    state: &SharedState<S>,
    ports: &S::Ports<'_>,
    ctx: &hypha_core::Ctx,
    id: PaymentId,
    action: PaymentAction,
    amount: Option<i64>,
    idempotency_key: Option<String>,
) -> Result<(StatusCode, Json<PaymentActionAcceptedResponse>), ApiError> {
    let input = ActOnPayment {
        payment: id,
        action,
        amount,
        idempotency_key,
    };
    let acted = act_on_payment(ctx, ports, &state.ids, &state.payments(ports), input).await?;
    let recorded = match acted.completed {
        true => record_completed(state, ports, &acted, action, amount).await,
        false => None,
    };
    let completed = recorded.is_some();
    let payment = recorded.unwrap_or(acted.payment);
    Ok((
        StatusCode::ACCEPTED,
        Json(PaymentActionAcceptedResponse {
            reference: acted.reference,
            completed,
            payment: PaymentResponse::new(&payment)?,
        }),
    ))
}

/// What a spore's provider did already (`ActionAnswer::Completed`: Stripe's capture
/// answered `succeeded`), recorded at once as its spore (design note 0030), so staff
/// see the payment move now; the provider's own report later changes nothing (the same
/// state, or the refund's ID counted). The payment as it is after; `None` (logged) when
/// it couldn't be recorded: the provider's report is the backstop.
async fn record_completed<S: Storage>(
    state: &SharedState<S>,
    ports: &S::Ports<'_>,
    acted: &hypha_payments::Acted,
    action: PaymentAction,
    amount: Option<i64>,
) -> Option<Payment> {
    use hypha_core::{ActingAs, Actor, Ctx, IdempotencyRequest, Scope};
    let payment = &acted.payment;
    let spore = hypha_apps::spore_of(
        hypha_apps::HasApps::apps(ports),
        payment.provider().as_str(),
    )
    .await
    .ok()
    .flatten()?;
    let left = payment.amount().amount() - payment.refunded().amount();
    let (kind, amount) = match action {
        PaymentAction::Capture => (PaymentEventKind::Paid, None),
        PaymentAction::Void => (PaymentEventKind::Voided, None),
        PaymentAction::Refund => match amount {
            Some(a) if a < left => (PaymentEventKind::PartiallyRefunded, Some(a)),
            Some(a) => (PaymentEventKind::Refunded, Some(a)),
            None => (PaymentEventKind::Refunded, None),
        },
    };
    let reference = acted.reference.clone();
    let key = format!(
        "{}:{action}:completed:{}",
        payment.id(),
        reference.as_deref().unwrap_or_default()
    );
    let fingerprint = {
        use sha2::Digest;
        sha2::Sha256::digest(format!("{kind:?}|{amount:?}|{reference:?}").as_bytes()).into()
    };
    let input = PaymentEvent {
        payment: payment.id(),
        kind,
        amount,
        reference,
        // The action ran on the payment's own attempt.
        attempt: None,
        occurred_at: None,
        idempotency: Some(IdempotencyRequest { key, fingerprint }),
    };
    let actor = Actor::App(spore.id());
    ports.act_as(actor);
    let ctx = Ctx::new(actor, vec![Scope::PaymentsWrite]);
    match record_payment_event(&ctx, ports, &state.clock, input).await {
        Ok(done) => done
            .value
            .payments
            .into_iter()
            .find(|p| p.id() == payment.id()),
        Err(e) => {
            tracing::warn!(payment = %payment.id(), "recording what the provider completed failed: {e}");
            None
        }
    }
}

/// Asks the payment's app to capture an authorized payment (`payments.capture`). 202:
/// the app accepted; the payment becomes `paid` when the app reports it, or at once (in
/// the answer's `payment`) when a provider built into the API says it's done (a spore,
/// design note 0030). Refused by the
/// provider: `409 payment_refused` with its `reason`. Not authorized: `409
/// invalid_payment_transition`; a `manual` payment: `409 payment_action_unsupported`; the
/// app gave no usable answer: `502 app_unavailable`. The `Idempotency-Key`, when sent,
/// is passed to the app (retry a timed-out request with the same one); without it each
/// request is a new attempt with its own key, so asking again after a refusal is asked
/// anew, not answered with the refusal.
#[utoipa::path(
    post,
    path = "/admin/payments/{id}/capture",
    params(("id" = PaymentId, Path), ("Idempotency-Key" = Option<String>, Header, description = "Passed to the app, so a retried action is done once. Without one, each request is a new attempt with its own key (`<payment>:<action>:<uuid>`).")),
    request_body = WholePaymentRequest,
    responses((status = 202, description = "Accepted", body = PaymentActionAcceptedResponse)),
    extensions(("x-hypha-scopes" = json!(["orders:write"]))),
)]
pub async fn capture<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<PaymentId>,
    req: KeyedJson<WholePaymentRequest>,
) -> Result<(StatusCode, Json<PaymentActionAcceptedResponse>), ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let key = req.parse()?.idempotency.map(|i| i.key);
    act(&state, &ports, &ctx, id, PaymentAction::Capture, None, key).await
}

/// Asks the payment's app to void a pending or authorized payment (`payments.void`),
/// releasing what it holds. 202 and the errors as for a capture.
#[utoipa::path(
    post,
    path = "/admin/payments/{id}/void",
    params(("id" = PaymentId, Path), ("Idempotency-Key" = Option<String>, Header, description = "Passed to the app, so a retried action is done once. Without one, each request is a new attempt with its own key (`<payment>:<action>:<uuid>`).")),
    request_body = WholePaymentRequest,
    responses((status = 202, description = "Accepted", body = PaymentActionAcceptedResponse)),
    extensions(("x-hypha-scopes" = json!(["orders:write"]))),
)]
pub async fn void<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<PaymentId>,
    req: KeyedJson<WholePaymentRequest>,
) -> Result<(StatusCode, Json<PaymentActionAcceptedResponse>), ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let key = req.parse()?.idempotency.map(|i| i.key);
    act(&state, &ports, &ctx, id, PaymentAction::Void, None, key).await
}

/// Asks the payment's app to refund some or all of a paid payment (`payments.refund`).
/// 202 and the errors as for a capture; an amount that doesn't fit is `422
/// invalid_refund_amount`. Send an `Idempotency-Key`: the app gives back once per key.
#[utoipa::path(
    post,
    path = "/admin/payments/{id}/refund",
    params(("id" = PaymentId, Path), ("Idempotency-Key" = Option<String>, Header, description = "Passed to the app, so a retried refund gives back once. Without one, each request is a new attempt with its own key.")),
    request_body = RefundRequest,
    responses((status = 202, description = "Accepted", body = PaymentActionAcceptedResponse)),
    extensions(("x-hypha-scopes" = json!(["orders:write"]))),
)]
pub async fn refund<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<PaymentId>,
    req: KeyedJson<RefundRequest>,
) -> Result<(StatusCode, Json<PaymentActionAcceptedResponse>), ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = req.parse()?;
    let key = req.idempotency.map(|i| i.key);
    let amount = req.body.amount;
    act(&state, &ports, &ctx, id, PaymentAction::Refund, amount, key).await
}
