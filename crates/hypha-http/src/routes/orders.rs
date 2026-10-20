use hypha_core::Stamped;

use super::stamp_times;
use axum::{Json, extract::State, http::StatusCode, response::Response};
use hypha_checkout::{Checkout, checkout};
use std::collections::HashMap;

use hypha_core::{
    AssetId, CartId, ChannelId, CountryCode, Currency, CustomerId, Email, Keyset, LocationId,
    MarketId, Money, OrderId, OrderLineId, PaymentId, ProductId, Scope, ShippingGroupId,
    ShippingOptionId, SortKey, TaxClassId, TaxRate, Timestamp, VariantId,
};
use hypha_media::{AssetRepo, HasAssets};
use hypha_order_actions::{
    CancelOrder, MarkPaid, OrderView, cancel_order, mark_paid as record_paid, order_view, view_of,
};
use hypha_orders::{
    CancelReason, FinancialStatus, FulfillmentStatus, HasOrders, ListOrders, Order,
    OrderFilterInput, OrderLine, OrderSort, OrderSummary, get_placed_order, list_orders,
};
use hypha_products::{HasProducts, ProductRepo};
use serde::{Deserialize, Serialize};

use hypha_apps::HasApps;
use hypha_core::Ctx;
use hypha_custom_fields::{HasCustomFieldDefinitions, HasCustomFieldValues, OwnerType};

use super::{
    address::PostalAddressResponse,
    custom_fields::{self, CustomFieldValues, FieldParams},
    page::{ListQuery, PageParams, PageResponse, SortedFilters},
    payments::{NextActionResponse, PaymentResponse, PlacedPaymentResponse},
    products::ProductMediaResponse,
    rfc3339,
    shipping::DeliveryEstimateDto,
};
use crate::{
    error::{ApiError, ApiErrorBody},
    extract::{Caller, JsonBody, KeyedJson, PathId, Query, Storefront, idempotent_response},
    media::Media,
    rate_limit::{self, Rule},
    redact::{self, Family, Redacted},
    state::{SharedState, Storage},
};

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CheckoutRequest {
    /// The total the buyer saw; if the cart now costs something else, checkout fails
    /// with `total_changed` (409) instead of placing a different order.
    #[serde(default)]
    expected_total: Option<Money>,
    /// Where a payment provider's page sends the buyer back (a redirect), passed to the
    /// payment's app.
    #[serde(default)]
    return_url: Option<String>,
}

/// `FinancialStatus` on the wire. A DTO of its own, so the domain enum carries no
/// serde and renaming a variant can't silently change the API.
/// Derived from the order's payments (design note 0027, PA7): manual payments use
/// `pending`, `paid` and `voided`; the rest come with providers that move money so.
#[derive(Clone, Copy, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum FinancialStatusDto {
    Pending,
    Authorized,
    PartiallyPaid,
    Paid,
    PartiallyRefunded,
    Refunded,
    Voided,
}

impl From<FinancialStatus> for FinancialStatusDto {
    fn from(s: FinancialStatus) -> Self {
        match s {
            FinancialStatus::Pending => FinancialStatusDto::Pending,
            FinancialStatus::Authorized => FinancialStatusDto::Authorized,
            FinancialStatus::PartiallyPaid => FinancialStatusDto::PartiallyPaid,
            FinancialStatus::Paid => FinancialStatusDto::Paid,
            FinancialStatus::PartiallyRefunded => FinancialStatusDto::PartiallyRefunded,
            FinancialStatus::Refunded => FinancialStatusDto::Refunded,
            FinancialStatus::Voided => FinancialStatusDto::Voided,
        }
    }
}

/// `FulfillmentStatus` on the wire.
#[derive(Clone, Copy, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum FulfillmentStatusDto {
    Unfulfilled,
    PartiallyFulfilled,
    Fulfilled,
}

impl From<FulfillmentStatus> for FulfillmentStatusDto {
    fn from(s: FulfillmentStatus) -> Self {
        match s {
            FulfillmentStatus::Unfulfilled => FulfillmentStatusDto::Unfulfilled,
            FulfillmentStatus::PartiallyFulfilled => FulfillmentStatusDto::PartiallyFulfilled,
            FulfillmentStatus::Fulfilled => FulfillmentStatusDto::Fulfilled,
        }
    }
}

/// `CancelReason` on the wire (design note 0018).
#[derive(Serialize, Deserialize, Clone, Copy, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CancelReasonDto {
    /// The buyer asked.
    Customer,
    /// The goods ran out or can't be shipped.
    Inventory,
    Fraud,
    /// Payment was declined.
    Declined,
    Staff,
    Other,
}

impl From<CancelReason> for CancelReasonDto {
    fn from(r: CancelReason) -> Self {
        match r {
            CancelReason::Customer => CancelReasonDto::Customer,
            CancelReason::Inventory => CancelReasonDto::Inventory,
            CancelReason::Fraud => CancelReasonDto::Fraud,
            CancelReason::Declined => CancelReasonDto::Declined,
            CancelReason::Staff => CancelReasonDto::Staff,
            CancelReason::Other => CancelReasonDto::Other,
        }
    }
}

impl From<CancelReasonDto> for CancelReason {
    fn from(r: CancelReasonDto) -> Self {
        match r {
            CancelReasonDto::Customer => CancelReason::Customer,
            CancelReasonDto::Inventory => CancelReason::Inventory,
            CancelReasonDto::Fraud => CancelReason::Fraud,
            CancelReasonDto::Declined => CancelReason::Declined,
            CancelReasonDto::Staff => CancelReason::Staff,
            CancelReasonDto::Other => CancelReason::Other,
        }
    }
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct OrderResponse {
    id: OrderId,
    /// The human number, shown as "#1001".
    number: i64,
    /// RFC 3339, UTC.
    placed_at: String,
    cart: CartId,
    market: MarketId,
    /// The sales channel it was placed through (design note 0020).
    channel: ChannelId,
    currency: Currency,
    country: CountryCode,
    customer: CustomerId,
    email: Email,
    /// Personal fields left out for this caller (design note 0029, AP10): an API key or
    /// app without the matching `customer_data:*` scope. They are absent from the
    /// object then, whatever the schema says. Never sent to staff or storefronts.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    redacted: Option<Vec<String>>,
    financial_status: FinancialStatusDto,
    fulfillment_status: FulfillmentStatusDto,
    /// Whether line prices include tax (the market decided, at checkout).
    taxes_included: bool,
    subtotal: Money,
    /// What shipping was charged, as part of `total`.
    shipping_total: Money,
    /// Tax on lines and shipping: part of `subtotal` and `shipping_total` if
    /// `taxes_included`, else on top.
    tax_total: Money,
    total: Money,
    lines: Vec<OrderLineResponse>,
    /// `null` when nothing in the order ships.
    shipping_address: Option<PostalAddressResponse>,
    /// How each group of lines ships, as charged.
    shipping_lines: Vec<OrderShippingLineResponse>,
    /// When it was created, RFC 3339 in UTC.
    created_at: String,
    /// When it last changed, RFC 3339 in UTC.
    updated_at: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct OrderShippingLineResponse {
    group: ShippingGroupId,
    /// The order lines this ships.
    lines: Vec<OrderLineId>,
    option: ShippingOptionId,
    code: String,
    name: String,
    /// `app_…` or `spore_…`; `null` for a built-in rate.
    provider: Option<String>,
    service: Option<String>,
    /// The provider's quote ID.
    reference: Option<String>,
    amount: Money,
    /// As estimated at checkout.
    estimate: Option<DeliveryEstimateDto>,
    /// `null` when the country had no tax rates.
    tax: Option<OrderTaxResponse>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct OrderLineResponse {
    id: OrderLineId,
    product: ProductId,
    variant: VariantId,
    quantity: u32,
    product_title: String,
    /// The variant's option values ("M / Blue"); `null` for a product without options.
    variant_title: Option<String>,
    sku: Option<String>,
    unit_price: Money,
    total: Money,
    /// `null` when the country had no tax rates.
    tax: Option<OrderTaxResponse>,
    /// How many of `quantity` standing fulfilments shipped.
    fulfilled_quantity: u32,
    /// The line's picture as its product is now (an order keeps none): the variant's
    /// image, else the product's first media item; `null` for a product gone or without
    /// media, or without file storage.
    image: Option<ProductMediaResponse>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct OrderTaxResponse {
    /// Parts per million: 250000 = 25%.
    rate_ppm: TaxRate,
    amount: Money,
    /// The class whose rate applied; `null` for the country's default rate.
    tax_class: Option<TaxClassId>,
}

impl OrderResponse {
    /// The response with each line's image (`line_images`).
    async fn with_images<P: HasProducts + HasAssets>(
        order: &Order,
        ports: &P,
        media: Option<&Media>,
    ) -> Result<Self, ApiError> {
        let mut response = Self::new(order)?;
        let mut images = line_images(order, ports, media).await?;
        for line in &mut response.lines {
            line.image = images.remove(&line.id);
        }
        Ok(response)
    }

    fn new(order: &Order) -> Result<Self, ApiError> {
        let (created_at, updated_at) = stamp_times(order.stamps())?;
        Ok(Self {
            created_at,
            updated_at,
            id: order.id(),
            number: order.number().value(),
            placed_at: rfc3339(order.placed_at())?,
            cart: order.cart(),
            market: order.market(),
            channel: order.channel(),
            currency: order.currency(),
            country: order.country(),
            customer: order.customer(),
            email: order.email().clone(),
            redacted: None,
            financial_status: order.financial_status().into(),
            fulfillment_status: order.fulfillment_status().into(),
            taxes_included: order.taxes_included(),
            subtotal: order.subtotal(),
            shipping_total: order.shipping_total(),
            tax_total: order.tax_total(),
            total: order.total(),
            lines: order.lines().iter().map(OrderLineResponse::from).collect(),
            shipping_address: order.shipping_address().map(PostalAddressResponse::from),
            shipping_lines: order
                .shipping_lines()
                .iter()
                .map(|s| OrderShippingLineResponse {
                    group: s.group,
                    lines: s.lines.clone(),
                    option: s.option,
                    code: s.code.clone(),
                    name: s.name.clone(),
                    provider: s.provider.clone(),
                    service: s.service.clone(),
                    reference: s.reference.clone(),
                    amount: s.amount.money(),
                    estimate: s
                        .estimate_days
                        .map(|(min_days, max_days)| DeliveryEstimateDto { min_days, max_days }),
                    tax: s.tax.map(|t| OrderTaxResponse {
                        rate_ppm: t.rate,
                        amount: t.amount,
                        tax_class: t.class,
                    }),
                })
                .collect(),
        })
    }
}

impl From<&OrderLine> for OrderLineResponse {
    fn from(l: &OrderLine) -> Self {
        Self {
            id: l.id,
            product: l.product,
            variant: l.variant,
            quantity: l.quantity.value(),
            product_title: l.product_title.clone(),
            variant_title: l.variant_title.clone(),
            sku: l.sku.clone(),
            unit_price: l.unit_price.money(),
            total: l.total,
            tax: l.tax.map(|t| OrderTaxResponse {
                rate_ppm: t.rate,
                amount: t.amount,
                tax_class: t.class,
            }),
            fulfilled_quantity: l.fulfilled_quantity,
            image: None,
        }
    }
}

/// Each line's image as its product is now: the variant's, else the product's first
/// media item, read in two queries for the order (`ProductRepo::images_of`, then the
/// assets). Lines whose product is gone, or has no media, get none; so does every line
/// without file storage.
async fn line_images<P: HasProducts + HasAssets>(
    order: &Order,
    ports: &P,
    media: Option<&Media>,
) -> Result<HashMap<OrderLineId, ProductMediaResponse>, ApiError> {
    let Some(media) = media else {
        return Ok(HashMap::new());
    };
    let mut asked: Vec<(ProductId, VariantId)> = order
        .lines()
        .iter()
        .map(|l| (l.product, l.variant))
        .collect();
    asked.sort();
    asked.dedup();
    let images = ports.products().images_of(&asked).await?;
    let wanted: Vec<(OrderLineId, AssetId)> = order
        .lines()
        .iter()
        .filter_map(|line| Some((line.id, *images.get(&(line.product, line.variant))?)))
        .collect();
    if wanted.is_empty() {
        return Ok(HashMap::new());
    }
    let assets: Vec<AssetId> = wanted.iter().map(|(_, a)| *a).collect();
    let found = ports.assets().get_many(&assets).await?;
    let mut images = HashMap::new();
    for (line, asset) in wanted {
        if let Some(asset) = found.iter().find(|a| a.id() == asset) {
            images.insert(line, ProductMediaResponse::new(asset, media).await?);
        }
    }
    Ok(images)
}

/// A storefront's view of an order it placed: the order with its access token, the key
/// to reading it again (`GET /storefront/orders/{id}?token=`; design note 0024, §7).
#[derive(Serialize, utoipa::ToSchema)]
pub struct PlacedOrderResponse {
    #[serde(flatten)]
    order: OrderResponse,
    /// Lets whoever holds it read this order: keep it with the buyer (a cookie, the
    /// confirmation email's link), never in a public place. The same for every read.
    access_token: String,
    /// How the buyer pays (design note 0027): the order's newest payment; `null` when
    /// there was nothing to pay.
    payment: Option<PlacedPaymentResponse>,
}

/// What checkout answers: the placed order and what the shopper must do next to pay.
#[derive(Serialize, utoipa::ToSchema)]
pub struct CheckoutResponse {
    #[serde(flatten)]
    placed: PlacedOrderResponse,
    /// For cash on delivery and bank transfer, the instructions (quote the order number
    /// as the reference). `null` when there was nothing to pay, for a payment made
    /// online (before the order), or when a manual provider couldn't start
    /// (`payment_error` says why).
    next_action: Option<NextActionResponse>,
    /// Why a manual method's payment couldn't start. `null` otherwise.
    payment_error: Option<ApiErrorBody>,
}

/// What the shopper must do next and why it's missing, as a placed order's answer
/// carries them: an error is logged and goes out as `payment_error`.
fn next_step(
    order: OrderId,
    next_action: &Option<Result<hypha_payments::NextAction, hypha_payments::PaymentError>>,
) -> (Option<NextActionResponse>, Option<ApiErrorBody>) {
    match next_action {
        Some(Ok(action)) => (Some(NextActionResponse::from(action)), None),
        Some(Err(error)) => {
            tracing::warn!(%order, %error, "the payment didn't start");
            let body = ApiError::from(hypha_payments::CommandError::from(error.clone())).body();
            (None, Some(body))
        }
        None => (None, None),
    }
}

/// How long checkout answers a checked-out cart with its order: an hour.
const REPLAY_WINDOW_MICROS: i64 = 60 * 60 * 1_000_000;

/// Places the order for the cart. Takes an `Idempotency-Key` header: a retry returns
/// the order the first request placed. The body is `{}` or `{"expected_total": …}`.
/// Something to pay needs a payment method chosen (`PUT …/payment-method`; 409
/// `payment_method_required`) and still offered (409 `payment_method_unavailable`). An
/// online method's money must be committed first (design note 0031: `POST
/// …/payment`): none started is 409 `payment_required`, not yet committed `payment_pending`
/// (its provider was asked; try again shortly, or it completes when the provider
/// reports), declined or released `payment_failed` (start paying again). If the order
/// can't be placed once the money is committed (a line sold out meanwhile), the money is
/// given back. A cart checked out already answers with its order for an hour after it
/// was placed (the provider's report may have completed it), then `409 cart_completed`.
#[utoipa::path(
    post,
    path = "/storefront/carts/{id}/checkout",
    params(("id" = CartId, Path), ("Idempotency-Key" = Option<String>, Header, description = "Makes the request safe to retry: a retry with the same key and body replays the first answer (marked `Idempotent-Replayed: true`) instead of repeating the effect. Same key with another body: `422 idempotency_key_reused`.")),
    request_body = CheckoutRequest,
    responses((status = 201, description = "Created", body = CheckoutResponse, headers(("Idempotent-Replayed" = String, description = "`true` when this answer replays an earlier request with the same `Idempotency-Key`."))), (status = 429, description = "Over the rate limit for this cart (`rate_limited`); see `Retry-After`", body = ApiErrorBody)),
    extensions(("x-hypha-scopes" = json!(["checkout"]))),
)]
pub async fn checkout_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller<Storefront>,
    PathId(cart): PathId<CartId>,
    req: KeyedJson<CheckoutRequest>,
) -> Result<Response, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    // Renewing an expired quote calls rate apps, like asking for options.
    rate_limit::enforce(&state.limiter, Rule::ShippingQuotesCart, &cart.to_string()).await?;
    let req = req.parse()?;
    let input = Checkout {
        cart,
        expected_total: req.body.expected_total,
        idempotency: req.idempotency,
        return_url: req.body.return_url,
    };
    let rates = state.cart_rates(&ports, cart);
    let providers = state.payments(&ports);
    let placed = checkout(
        &ctx,
        &ports,
        &state.ids,
        &state.clock,
        &rates,
        &providers,
        input,
    )
    .await?;
    let order = &placed.value.order;
    // A cart checked out already answers with its order and access token for an hour
    // (the shopper's browser after the provider's webhook placed it; a retry), then only
    // says so: an old cart ID isn't a key to the order (design note 0031).
    let age = hypha_core::Clock::now(&state.clock).unix_micros() - order.placed_at().unix_micros();
    if placed.replayed && age > REPLAY_WINDOW_MICROS {
        return Err(hypha_checkout::CommandError::CartCompleted {
            cart,
            order: order.id(),
        }
        .into());
    }
    let (next_action, payment_error) = next_step(order.id(), &placed.value.next_action);
    Ok(idempotent_response(
        StatusCode::CREATED,
        CheckoutResponse {
            placed: PlacedOrderResponse {
                order: OrderResponse::with_images(order, &ports, state.media.as_deref()).await?,
                access_token: state.env.order_token(order.id()),
                payment: placed
                    .value
                    .payment
                    .as_ref()
                    .map(PlacedPaymentResponse::new),
            },
            next_action,
            payment_error,
        },
        placed.replayed,
    ))
}

#[derive(Deserialize, utoipa::IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct OrderTokenQuery {
    /// The order's `access_token`, from checkout's answer.
    token: String,
}

/// An order this storefront placed, for its confirmation page, with its access token: a
/// missing or wrong token, or another channel's order, is `404 order_not_found` (as if
/// it didn't exist).
#[utoipa::path(
    get,
    path = "/storefront/orders/{id}",
    params(("id" = OrderId, Path), OrderTokenQuery),
    responses((status = 200, description = "OK", body = PlacedOrderResponse)),
    extensions(("x-hypha-scopes" = json!(["checkout"]))),
)]
pub async fn fetch_placed<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller<Storefront>,
    PathId(id): PathId<OrderId>,
    query: Query<OrderTokenQuery>,
) -> Result<Json<PlacedOrderResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let token = query.parse()?.token;
    if !state.env.is_order_token(id, &token) {
        return Err(hypha_orders::CommandError::OrderNotFound(id).into());
    }
    let order = get_placed_order(&ctx, ports.orders(), id).await?;
    let payment = hypha_payments::newest_payment(&ports, id).await?;
    Ok(Json(PlacedOrderResponse {
        order: OrderResponse::with_images(&order, &ports, state.media.as_deref()).await?,
        access_token: token,
        payment: payment.as_ref().map(PlacedPaymentResponse::new),
    }))
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct StartCartPaymentRequest {
    /// The total the shopper sees; if the cart now costs something else, `409
    /// total_changed` instead of collecting a different amount.
    #[serde(default)]
    expected_total: Option<Money>,
    /// Where a payment provider's page sends the shopper back (a redirect): a page that
    /// checks the cart out (`POST /storefront/carts/{id}/checkout`).
    #[serde(default)]
    return_url: Option<String>,
}

/// A cart's payment, started (design note 0031).
#[derive(Serialize, utoipa::ToSchema)]
pub struct CartPaymentResponse {
    /// The payment (`pay_…`): what the provider's reports name.
    payment: PaymentId,
    /// What it collects: the cart's total.
    amount: Money,
    status: crate::routes::payments::PaymentStatusDto,
    /// The provider's card form (`client_widget`: mount its element with these props on
    /// the checkout, and check out on its `hypha:payment-confirmed`) or page
    /// (`redirect`). `null` when the provider couldn't start (`payment_error`).
    next_action: Option<NextActionResponse>,
    /// Why the provider couldn't start (an app that's down: `app_unavailable`): start
    /// again. `null` otherwise.
    payment_error: Option<ApiErrorBody>,
}

/// Starts paying for the cart before its order exists (design note 0031): an online
/// method's money is committed first, and checkout places the order once it is. Runs
/// checkout's checks (409 `total_changed`, `payment_method_required`, …). A pending
/// payment of the same method and total is started again (the provider resumes it);
/// otherwise a new one is made and the old ones released. Refused for a manual method
/// (409 `payment_method_offline`: check out instead), a total of zero (409
/// `cart_nothing_to_pay`) and a cart whose money is committed already (409
/// `cart_payment_committed`: check out).
#[utoipa::path(
    post,
    path = "/storefront/carts/{id}/payment",
    params(("id" = CartId, Path)),
    request_body = StartCartPaymentRequest,
    responses((status = 200, description = "OK", body = CartPaymentResponse), (status = 429, description = "Over the rate limit for this cart (`rate_limited`); see `Retry-After`", body = ApiErrorBody)),
    extensions(("x-hypha-scopes" = json!(["checkout"]))),
)]
pub async fn start_cart_payment<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller<Storefront>,
    PathId(cart): PathId<CartId>,
    body: JsonBody<StartCartPaymentRequest>,
) -> Result<Json<CartPaymentResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    // Its checks may renew a quote, which calls rate apps, like checkout.
    rate_limit::enforce(&state.limiter, Rule::ShippingQuotesCart, &cart.to_string()).await?;
    let req = body.parse()?;
    let input = hypha_checkout::StartCartPayment {
        cart,
        expected_total: req.expected_total,
        return_url: req.return_url,
    };
    let rates = state.cart_rates(&ports, cart);
    let providers = state.payments(&ports);
    let started = hypha_checkout::start_cart_payment(
        &ctx,
        &ports,
        &state.ids,
        &state.clock,
        &rates,
        &providers,
        input,
    )
    .await?;
    let payment = &started.payment;
    let (next_action, payment_error) = match &started.next_action {
        Ok(action) => (Some(NextActionResponse::from(action)), None),
        Err(error) => {
            tracing::warn!(payment = %payment.id(), %error, "the cart's payment didn't start");
            let body = ApiError::from(hypha_payments::CommandError::from(error.clone())).body();
            (None, Some(body))
        }
    };
    Ok(Json(CartPaymentResponse {
        payment: payment.id(),
        amount: payment.amount(),
        status: payment.status().into(),
        next_action,
        payment_error,
    }))
}

/// An order in a list: what a table of orders shows, without lines.
#[derive(Serialize, utoipa::ToSchema)]
pub struct OrderSummaryResponse {
    id: OrderId,
    /// The human number, shown as "#1001".
    number: i64,
    /// RFC 3339, UTC.
    placed_at: String,
    /// The sales channel it was placed through (design note 0020).
    channel: ChannelId,
    customer: CustomerId,
    email: Email,
    /// Personal fields left out for this caller (design note 0029, AP10): an API key or
    /// app without the matching `customer_data:*` scope. They are absent from the
    /// object then, whatever the schema says. Never sent to staff or storefronts.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    redacted: Option<Vec<String>>,
    country: CountryCode,
    financial_status: FinancialStatusDto,
    fulfillment_status: FulfillmentStatusDto,
    /// When it was cancelled, RFC 3339 in UTC; `null` if it wasn't.
    cancelled_at: Option<String>,
    total: Money,
    /// How many items, adding up line quantities.
    item_count: u64,
    /// The custom field values `?custom_fields=` asked for (design note 0033); absent
    /// without it.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<std::collections::HashMap<String, serde_json::Value>>, required = false)]
    custom_fields: Option<CustomFieldValues>,
}

impl OrderSummaryResponse {
    fn new(o: OrderSummary, custom_fields: Option<CustomFieldValues>) -> Result<Self, ApiError> {
        Ok(Self {
            id: o.id,
            number: o.number.value(),
            placed_at: rfc3339(o.placed_at)?,
            channel: o.channel,
            customer: o.customer,
            email: o.email,
            redacted: None,
            country: o.country,
            financial_status: o.financial_status.into(),
            fulfillment_status: o.fulfillment_status.into(),
            cancelled_at: o.cancelled_at.map(rfc3339).transpose()?,
            total: o.total,
            item_count: o.item_count,
            custom_fields,
        })
    }
}

/// A page of orders (named for the documents: utoipa's `body =` takes a plain name).
type OrderPage = PageResponse<OrderSummaryResponse>;

impl From<FinancialStatusDto> for FinancialStatus {
    fn from(s: FinancialStatusDto) -> Self {
        match s {
            FinancialStatusDto::Pending => FinancialStatus::Pending,
            FinancialStatusDto::Authorized => FinancialStatus::Authorized,
            FinancialStatusDto::PartiallyPaid => FinancialStatus::PartiallyPaid,
            FinancialStatusDto::Paid => FinancialStatus::Paid,
            FinancialStatusDto::PartiallyRefunded => FinancialStatus::PartiallyRefunded,
            FinancialStatusDto::Refunded => FinancialStatus::Refunded,
            FinancialStatusDto::Voided => FinancialStatus::Voided,
        }
    }
}

impl From<FulfillmentStatusDto> for FulfillmentStatus {
    fn from(s: FulfillmentStatusDto) -> Self {
        match s {
            FulfillmentStatusDto::Unfulfilled => FulfillmentStatus::Unfulfilled,
            FulfillmentStatusDto::PartiallyFulfilled => FulfillmentStatus::PartiallyFulfilled,
            FulfillmentStatusDto::Fulfilled => FulfillmentStatus::Fulfilled,
        }
    }
}

#[derive(Deserialize, utoipa::IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct OrderFilters {
    /// The order's number (`1001` or `#1001`), or text its email contains, ignoring case.
    /// Up to 255 characters; blank filters nothing. Searching by email takes
    /// `customer_data:email` for callers that aren't staff sessions.
    q: Option<String>,
    /// Only orders in this payment status; repeat for any of several.
    #[serde(default)]
    financial_status: Vec<FinancialStatusDto>,
    /// Only orders in this fulfilment status; repeat for any of several.
    #[serde(default)]
    fulfillment_status: Vec<FulfillmentStatusDto>,
    /// Only cancelled orders (`true`), or only those that aren't (`false`).
    cancelled: Option<bool>,
    /// Only orders placed through this sales channel.
    #[param(value_type = Option<ChannelId>)]
    channel: Option<ChannelId>,
    /// Only orders shipped to (or placed in) this country (ISO 3166-1 alpha-2).
    country: Option<String>,
    /// Only orders in this currency (ISO 4217); needed to sort by total.
    currency: Option<String>,
    /// Only orders placed at or after this instant (RFC 3339).
    placed_from: Option<String>,
    /// Only orders placed before this instant (RFC 3339).
    placed_before: Option<String>,
    #[param(inline)]
    sort: Option<OrderSortDto>,
}

/// The order of an order list (design note 0024, AU1). Default `newest`. Sorting by
/// total needs the `currency` filter (`422 sort_needs_currency`); ties go by placing
/// order, in the same direction.
#[derive(Debug, Clone, Copy, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum OrderSortDto {
    Newest,
    Oldest,
    TotalDesc,
    TotalAsc,
}

impl OrderFilters {
    fn sort(&self) -> OrderSort {
        match self.sort {
            None | Some(OrderSortDto::Newest) => OrderSort::Newest,
            Some(OrderSortDto::Oldest) => OrderSort::Oldest,
            Some(OrderSortDto::TotalDesc) => OrderSort::TotalDesc,
            Some(OrderSortDto::TotalAsc) => OrderSort::TotalAsc,
        }
    }

    fn input(self) -> Result<OrderFilterInput, ApiError> {
        let instant = |name: &str, value: Option<String>| {
            value
                .map(|v| {
                    v.parse::<jiff::Timestamp>()
                        .map(|t| Timestamp::from_unix_micros(t.as_microsecond()))
                        .map_err(|e| {
                            ApiError::invalid_query(format!("`{name}` is an RFC 3339 instant: {e}"))
                        })
                })
                .transpose()
        };
        Ok(OrderFilterInput {
            q: self.q,
            financial_statuses: self.financial_status.into_iter().map(Into::into).collect(),
            fulfillment_statuses: self
                .fulfillment_status
                .into_iter()
                .map(Into::into)
                .collect(),
            cancelled: self.cancelled,
            channel: self.channel,
            country: self.country.as_deref().map(CountryCode::new).transpose()?,
            currency: self.currency.as_deref().map(Currency::new).transpose()?,
            placed_from: instant("placed_from", self.placed_from)?,
            placed_before: instant("placed_before", self.placed_before)?,
        })
    }
}

impl SortedFilters for OrderFilters {
    fn fits(&self, key: &SortKey) -> bool {
        self.sort().fits(key)
    }
}

/// Orders, newest first or in `sort`'s order, narrowed by the filters (every one given
/// must match), custom fields' among them.
#[utoipa::path(
    get,
    path = "/admin/orders",
    params(PageParams, OrderFilters, FieldParams),
    responses((status = 200, description = "OK", body = OrderPage)),
    extensions(("x-hypha-scopes" = json!(["orders:read"]))),
)]
pub async fn list<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    query: ListQuery<OrderFilters>,
) -> Result<Json<Redacted>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let (page, filters, scope, field_query) = query.parse_sorted_with_fields()?;
    let fields = custom_fields::list_fields(&ctx, &ports, OwnerType::Order, &field_query).await?;
    // Text other than a number searches emails: only for callers who may see them
    // (design note 0029, AP10).
    if let Some(q) = filters.q.as_deref().map(str::trim)
        && !q.is_empty()
        && !q
            .trim_start_matches('#')
            .chars()
            .all(|c| c.is_ascii_digit())
    {
        redact::require_fields(&ctx, &[Scope::CustomerDataEmail])?;
    }
    let sort = filters.sort();
    let input = ListOrders {
        filter: filters.input()?,
        sort,
        fields: fields.filters,
    };
    let page = list_orders(&ctx, ports.orders(), input, page).await?;
    let values = match &fields.keys {
        Some(keys) => {
            let owners: Vec<uuid::Uuid> = page.items.iter().map(|o| o.id.as_uuid()).collect();
            Some(
                custom_fields::read(
                    &ctx,
                    &fields.kind,
                    &ports,
                    &[OwnerType::Order],
                    &owners,
                    Some(keys),
                )
                .await?,
            )
        }
        None => None,
    };
    let items = page
        .items
        .iter()
        .cloned()
        .map(|o| {
            let custom = values.as_ref().map(|v| v.json_of(o.id.as_uuid()));
            OrderSummaryResponse::new(o, custom)
        })
        .collect::<Result<_, _>>()?;
    let page = PageResponse::sorted(
        &page,
        &scope,
        |o| Keyset {
            key: sort.key(o),
            id: o.id,
        },
        items,
    )?;
    let hidden = values.map(|v| v.redacted).unwrap_or_default();
    Ok(Json(redact::page_with_fields(
        &ctx,
        Family::Order,
        page,
        &hidden,
    )?))
}

/// Units of a line still to ship, and where they wait (design note 0018, FU4).
#[derive(Serialize, utoipa::ToSchema)]
pub struct UnfulfilledDto {
    line: OrderLineId,
    /// The location holding them for the line; `null` for units held nowhere (an
    /// untracked item): any location.
    location: Option<LocationId>,
    quantity: u32,
}

/// An order as the admin sees it: with its cancellation and what's left to ship, by
/// location. A storefront's `OrderResponse` has neither: why staff cancelled an order
/// (`fraud`) is theirs to know.
#[derive(Serialize, utoipa::ToSchema)]
pub struct AdminOrderResponse {
    #[serde(flatten)]
    order: OrderResponse,
    /// When it was cancelled, RFC 3339 in UTC; `null` if it wasn't.
    cancelled_at: Option<String>,
    /// Why it was cancelled; `null` if it wasn't.
    cancel_reason: Option<CancelReasonDto>,
    /// Per line with anything left, in line order; empty once everything shipped or the
    /// order was cancelled.
    unfulfilled: Vec<UnfulfilledDto>,
    /// Its payments, newest first (design note 0027): the method, status and
    /// instructions of each.
    payments: Vec<PaymentResponse>,
    /// Its custom field values the caller may read (design note 0033); `PATCH
    /// /admin/orders/{id}/custom-fields` writes them. A personal one is left out
    /// without `customer_data:profile` and listed in `redacted` as
    /// `custom_fields.<key>`.
    #[schema(value_type = std::collections::HashMap<String, serde_json::Value>)]
    custom_fields: CustomFieldValues,
}

impl AdminOrderResponse {
    /// `order` is `view.order`'s response, with its images (`OrderResponse::with_images`).
    fn new(
        view: &OrderView,
        order: OrderResponse,
        custom_fields: CustomFieldValues,
    ) -> Result<Self, ApiError> {
        let cancelled = view.order.cancelled();
        Ok(Self {
            order,
            cancelled_at: cancelled.map(|c| rfc3339(c.at)).transpose()?,
            cancel_reason: cancelled.map(|c| c.reason.into()),
            unfulfilled: view
                .unfulfilled
                .iter()
                .map(|u| UnfulfilledDto {
                    line: u.line,
                    location: u.location,
                    quantity: u.quantity,
                })
                .collect(),
            payments: PaymentResponse::all(&view.payments)?,
            custom_fields,
        })
    }

    /// The response for `view` (its `order` already made), with its custom fields, as
    /// `ctx` may see it.
    async fn respond<P>(
        ctx: &Ctx,
        ports: &P,
        view: &OrderView,
        order: OrderResponse,
    ) -> Result<Redacted, ApiError>
    where
        P: HasApps + HasCustomFieldDefinitions + HasCustomFieldValues,
    {
        let kind = custom_fields::caller_kind(ctx, ports).await?;
        let id = view.order.id().as_uuid();
        let fields =
            custom_fields::read(ctx, &kind, ports, &[OwnerType::Order], &[id], None).await?;
        let dto = Self::new(view, order, fields.json_of(id))?;
        redact::one_with_fields(ctx, Family::Order, dto, &fields.redacted)
    }
}

/// An order, with what's left to ship.
#[utoipa::path(
    get,
    path = "/admin/orders/{id}",
    params(("id" = OrderId, Path)),
    responses((status = 200, description = "OK", body = AdminOrderResponse)),
    extensions(("x-hypha-scopes" = json!(["orders:read"]))),
)]
pub async fn fetch<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<OrderId>,
) -> Result<Json<Redacted>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let view = order_view(&ctx, &ports, id).await?;
    let order = OrderResponse::with_images(&view.order, &ports, state.media.as_deref()).await?;
    Ok(Json(
        AdminOrderResponse::respond(&ctx, &ports, &view, order).await?,
    ))
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CancelOrderRequest {
    reason: CancelReasonDto,
    /// Email the buyer that the order is cancelled (`false` if absent).
    #[serde(default)]
    notify: bool,
}

/// Cancels the order: every line's reserved stock is released and its pending payments
/// are voided. Refused while fulfilments stand (`order_has_fulfillments`, 409, listing
/// them): cancel those first. Refused while a payment is authorized
/// (`payment_authorized`, 409, `details.payment`): the held money is its provider's to
/// release. A paid order stays paid; refund it outside Hypha for now. Cancelling it
/// again changes nothing.
#[utoipa::path(
    post,
    path = "/admin/orders/{id}/cancel",
    params(("id" = OrderId, Path)),
    request_body = CancelOrderRequest,
    responses((status = 200, description = "OK", body = AdminOrderResponse)),
    extensions(("x-hypha-scopes" = json!(["orders:write"]))),
)]
pub async fn cancel<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<OrderId>,
    body: JsonBody<CancelOrderRequest>,
) -> Result<Json<Redacted>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let input = CancelOrder {
        order: id,
        reason: req.reason.into(),
        // The event carries it; the mailer answers `order.cancelled` (design note 0028,
        // NT7). Cancelling again records no event, so the buyer is told once.
        notify_customer: req.notify,
    };
    let (done, _cancelled_now) = cancel_order(&ctx, &ports, &state.clock, input).await?;
    // A cancelled order has nothing left to ship.
    let response = OrderResponse::with_images(&done.order, &ports, state.media.as_deref()).await?;
    let view = OrderView {
        order: done.order,
        unfulfilled: Vec::new(),
        payments: done.payments,
    };
    Ok(Json(
        AdminOrderResponse::respond(&ctx, &ports, &view, response).await?,
    ))
}

#[derive(Deserialize, Default, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct MarkPaidRequest {
    /// What staff have to show for it (a bank reference, the courier's receipt), up to
    /// 255 characters; kept on the payment.
    #[serde(default)]
    reference: Option<String>,
}

/// Records that the order's money arrived (a bank transfer, cash from the courier;
/// design note 0027, §5): its pending payments become `paid`, with the `reference` when
/// given, and so does the order. A `pending` order without an open payment gets a
/// `manual` one of its total, paid on the spot. The body is optional (none, `{}` or
/// `{"reference": …}`). Marking a paid order again changes nothing. Refused (409): a
/// cancelled order (`order_cancelled`), an authorized payment (`payment_authorized`,
/// `details.payment`: only its provider can capture it), and any other payment status
/// (`order_not_payable`, `details.financial_status`: partially paid, refunded or
/// voided, where money moved already).
#[utoipa::path(
    post,
    path = "/admin/orders/{id}/paid",
    params(("id" = OrderId, Path)),
    request_body(content = Option<MarkPaidRequest>, description = "Optional."),
    responses((status = 200, description = "OK", body = AdminOrderResponse)),
    extensions(("x-hypha-scopes" = json!(["orders:write"]))),
)]
pub async fn mark_paid<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<OrderId>,
    body: JsonBody<Option<MarkPaidRequest>>,
) -> Result<Json<Redacted>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse_optional()?.flatten().unwrap_or_default();
    let input = MarkPaid {
        order: id,
        reference: req.reference,
    };
    let done = record_paid(&ctx, &ports, &state.ids, &state.clock, input).await?;
    let view = view_of(&ports, done.order, done.payments).await?;
    let order = OrderResponse::with_images(&view.order, &ports, state.media.as_deref()).await?;
    Ok(Json(
        AdminOrderResponse::respond(&ctx, &ports, &view, order).await?,
    ))
}
