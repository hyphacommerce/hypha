//! Payment methods (design note 0027, §7): how shoppers can pay, as settings, and the
//! payments shown on orders.

use axum::{Json, extract::State, http::StatusCode};
use hypha_core::{ChannelId, MarketId, Money, PaymentId, PaymentMethodId, Stamped};
use hypha_payments::{
    CreatePaymentMethod, NextAction, Payment, PaymentMethod, PaymentStatus, UpdatePaymentMethod,
    create_payment_method, delete_payment_method, get_payment_method, list_payment_methods,
    update_payment_method,
};
use serde::{Deserialize, Serialize};

use super::{rfc3339, stamp_times};
use crate::{
    error::ApiError,
    extract::{Caller, JsonBody, PathId},
    patch,
    state::{SharedState, Storage},
};

#[derive(Serialize, utoipa::ToSchema)]
pub struct PaymentMethodResponse {
    id: PaymentMethodId,
    /// `manual` (cash on delivery, bank transfer: staff record the money), or a spore's
    /// `spore_<name>`.
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
    /// The provider's own settings (`{}` for `manual`).
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

fn default_true() -> bool {
    true
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreatePaymentMethodRequest {
    /// `manual`, or a spore's `spore_<name>` this build has (`payment_provider_unknown`
    /// otherwise).
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
    /// The provider's settings, checked by it (`invalid_payment_settings`); `manual`
    /// takes none (`{}`, the default).
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
    let providers = state.storage.payment_providers();
    let method = create_payment_method(&ctx, &ports, &state.ids, providers, input).await?;
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
    let providers = state.storage.payment_providers();
    let method = update_payment_method(&ctx, &ports, providers, id, input)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("payment_method_not_found"))?;
    Ok(Json(PaymentMethodResponse::new(&method)?))
}

/// Deletes a payment method. One a pending payment uses is refused
/// (`payment_method_in_use`, 409): disable it instead. Payments it made keep its name.
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

/// A payment as the admin sees it on an order.
#[derive(Serialize, utoipa::ToSchema)]
pub struct PaymentResponse {
    id: PaymentId,
    /// The method it was made with; `null` for money staff recorded on an order placed
    /// without one.
    method: Option<PaymentMethodId>,
    /// The method's name when the payment was made.
    method_name: Option<String>,
    /// `manual`, or a spore's key.
    provider: String,
    amount: Money,
    status: PaymentStatusDto,
    /// What was given back; zero until refunds exist.
    refunded: Money,
    /// The provider's ID, or for manual payments what staff typed (a bank reference).
    reference: Option<String>,
    /// The method's instructions at checkout, as the shopper was shown them.
    instructions: Option<String>,
    /// When the money was collected, RFC 3339 in UTC.
    paid_at: Option<String>,
    /// When it was voided, RFC 3339 in UTC.
    voided_at: Option<String>,
    /// When it was created, RFC 3339 in UTC.
    created_at: String,
}

impl PaymentResponse {
    pub(crate) fn new(p: &Payment) -> Result<Self, ApiError> {
        let (created_at, _) = stamp_times(p.stamps())?;
        Ok(Self {
            id: p.id(),
            method: p.method(),
            method_name: p.method_name().map(str::to_string),
            provider: p.provider().to_string(),
            amount: p.amount(),
            status: p.status().into(),
            refunded: p.refunded(),
            reference: p.reference().map(str::to_string),
            instructions: p.instructions().map(str::to_string),
            paid_at: p.paid_at().map(rfc3339).transpose()?,
            voided_at: p.voided_at().map(rfc3339).transpose()?,
            created_at,
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

/// What the shopper must do next to pay (design note 0027, §3).
#[derive(Serialize, utoipa::ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NextActionResponse {
    /// Nothing online: show the instructions (cash on delivery, bank transfer). Empty
    /// when the method has none.
    Instructions { instructions: String },
    /// A provider's widget (card fields) takes this secret.
    ClientSecret { client_secret: String },
    /// Send the shopper to the provider's page.
    Redirect { url: String },
}

impl From<&NextAction> for NextActionResponse {
    fn from(a: &NextAction) -> Self {
        match a {
            NextAction::Instructions(instructions) => NextActionResponse::Instructions {
                instructions: instructions.clone(),
            },
            NextAction::ClientSecret { secret } => NextActionResponse::ClientSecret {
                client_secret: secret.clone(),
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
    /// `manual` or a spore's key: what the storefront shows after choosing (instructions,
    /// or a provider's widget).
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
