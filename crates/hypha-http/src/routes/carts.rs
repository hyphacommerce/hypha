use axum::{Json, extract::State, http::StatusCode, response::Response};
use hypha_cart::{
    AddLine, CreateCart, GroupOffers, LineStatus, PricedCart, PricedShippingGroup, SetLineQuantity,
    add_line, create_cart, remove_cart_line, select_shipping, set_cart_customer, set_cart_email,
    set_line_quantity, set_shipping_address, shipping_offers,
};
use hypha_core::{
    AssetId, CartId, CountryCode, Currency, CustomerId, Email, ErrorCode, MarketId, Money, OrderId,
    PaymentMethodId, ProductId, ShippingGroupId, ShippingOptionId, TaxClassId, TaxRate, VariantId,
};
use hypha_media::{AssetRepo, HasAssets};
use hypha_shipping::OfferOutcome;
use hypha_tax::TaxLine;
use serde::{Deserialize, Serialize};

use super::{
    address::{PostalAddressRequest, PostalAddressResponse},
    payments::CartPaymentMethodResponse,
    products::ProductMediaResponse,
    rfc3339,
    shipping::DeliveryEstimateDto,
};
use crate::{
    client_ip::ClientIp,
    error::{ApiError, ApiErrorBody},
    extract::{Caller, JsonBody, KeyedJson, PathId, Storefront, idempotent_response},
    media::Media,
    rate_limit::{self, Rule},
    state::{SharedState, Storage},
};

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateCartRequest {
    /// Where the cart ships to; picks the market and the tax rates.
    country: String,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AddLineRequest {
    variant: VariantId,
    quantity: u32,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetEmailRequest {
    /// `null` clears it.
    email: Option<String>,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetCustomerRequest {
    /// `null` unlinks the customer (the cart keeps their email).
    customer: Option<CustomerId>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct CartResponse {
    id: CartId,
    /// Picked from the country when the cart was created; it sets the currency.
    market: MarketId,
    /// The currency of every amount in the cart.
    currency: Currency,
    country: CountryCode,
    /// Who to contact about the order; required at checkout.
    email: Option<Email>,
    /// Personal fields left out for this caller (design note 0029, AP10): an API key or
    /// app without the matching `customer_data:*` scope. They are absent from the
    /// object then, whatever the schema says. Never sent to staff or storefronts.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    redacted: Option<Vec<String>>,
    customer: Option<CustomerId>,
    /// The order this cart was checked out as; `null` while it can still change.
    order: Option<OrderId>,
    /// Whether line prices include tax (set on the market).
    taxes_included: bool,
    /// Sum of the available lines, as priced. All totals count available lines only.
    subtotal: Money,
    /// The chosen shipping, priced now; zero until chosen. Part of `total`.
    shipping_total: Money,
    /// Tax on the available lines and on shipping: part of `subtotal` and
    /// `shipping_total` if `taxes_included`, else on top.
    tax_total: Money,
    /// What the buyer pays.
    total: Money,
    /// False when any line is unavailable; checkout should refuse such a cart.
    fully_available: bool,
    lines: Vec<LineResponse>,
    /// Where it ships: in the cart's country. Required at checkout when something ships.
    shipping_address: Option<PostalAddressResponse>,
    /// The groups of lines that ship together, each needing a chosen option before
    /// checkout; empty when nothing ships.
    shipping_groups: Vec<ShippingGroupResponse>,
    /// Whether every group has an option priced now (checkout requires it).
    shipping_ready: bool,
    /// How the buyer chose to pay (`PUT …/payment-method`); checkout requires one when
    /// there's something to pay.
    payment_method: Option<PaymentMethodId>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct ShippingGroupResponse {
    /// Stable for the cart: what `PUT …/shipping/{group_id}` names.
    id: ShippingGroupId,
    /// The available lines that ship in this group.
    lines: Vec<VariantId>,
    /// The buyer's choice at its price now; `null` until chosen, or when `problem` is set.
    selected: Option<SelectedShippingResponse>,
    /// Why the chosen option can't be charged now; pick again from the offers.
    /// `shipping_option_unavailable`, `no_price_in_currency`, `weight_unknown`, or a money
    /// error code.
    problem: Option<&'static str>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct SelectedShippingResponse {
    option: ShippingOptionId,
    code: String,
    name: String,
    amount: Money,
    /// `null` when the cart's country has no tax rates.
    tax: Option<TaxLineResponse>,
    estimate: Option<DeliveryEstimateDto>,
    /// RFC 3339: for a provider's rate, when checkout asks the provider again (and may
    /// answer `total_changed`). `null` for built-in rates, which are always current.
    quote_expires_at: Option<String>,
}

impl From<&PricedShippingGroup> for ShippingGroupResponse {
    fn from(g: &PricedShippingGroup) -> Self {
        Self {
            id: g.id,
            lines: g.lines.clone(),
            selected: g.selected.as_ref().map(|s| SelectedShippingResponse {
                option: s.option,
                code: s.code.clone(),
                name: s.name.clone(),
                amount: s.amount,
                tax: s.tax.as_ref().map(TaxLineResponse::from),
                estimate: s.estimate.map(Into::into),
                // A clock reading plus 15 minutes is always in RFC 3339's range.
                quote_expires_at: s.quote.as_ref().and_then(|q| rfc3339(q.expires_at()).ok()),
            }),
            problem: g.problem.as_ref().map(|p| p.code()),
        }
    }
}

#[derive(Serialize, utoipa::ToSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum LineState {
    Priced {
        unit_price: Money,
        total: Money,
        /// `null` when the cart's country has no tax rates.
        tax: Option<TaxLineResponse>,
    },
    Unavailable {
        /// Stable `ErrorCode`, e.g. "product_not_active".
        reason_code: &'static str,
        reason: String,
    },
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct TaxLineResponse {
    /// Parts per million: 250000 = 25%.
    rate_ppm: TaxRate,
    amount: Money,
    /// The class whose rate applied; `null` for the country's default rate.
    tax_class: Option<TaxClassId>,
}

impl From<&TaxLine> for TaxLineResponse {
    fn from(t: &TaxLine) -> Self {
        Self {
            rate_ppm: t.rate,
            amount: t.amount,
            tax_class: t.class,
        }
    }
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct LineResponse {
    product: ProductId,
    variant: VariantId,
    quantity: u32,
    /// The product's title now; `null` only when the product or variant is gone, so an
    /// unavailable line still says what it was.
    product_title: Option<String>,
    /// The variant's option values ("M / Blue"); `null` for a product without options.
    variant_title: Option<String>,
    sku: Option<String>,
    /// The product's handle, to link to its page; `null` when the product is gone or the
    /// cart is checked out.
    handle: Option<String>,
    /// The variant's image, else the product's first media item; `null` without one,
    /// once checked out, or where the deployment has no file storage.
    image: Option<ProductMediaResponse>,
    /// How many of the variant can be bought now, so a quantity input can stop there:
    /// `null` when there's no limit (stock isn't tracked, or it sells past zero).
    available_quantity: Option<u64>,
    #[serde(flatten)]
    state: LineState,
}

impl From<&PricedCart> for CartResponse {
    fn from(cart: &PricedCart) -> Self {
        Self {
            id: cart.id,
            market: cart.market,
            currency: cart.currency,
            country: cart.country,
            email: cart.email.clone(),
            redacted: None,
            customer: cart.customer,
            order: cart.order,
            taxes_included: cart.taxes_included,
            subtotal: cart.subtotal,
            shipping_total: cart.shipping_total,
            tax_total: cart.tax_total,
            total: cart.total,
            fully_available: cart.is_fully_available(),
            shipping_address: cart
                .shipping_address
                .as_ref()
                .map(PostalAddressResponse::from),
            shipping_groups: cart.shipping_groups.iter().map(Into::into).collect(),
            shipping_ready: cart.is_shipping_ready(),
            payment_method: cart.payment_method,
            lines: cart
                .lines
                .iter()
                .map(|l| LineResponse {
                    product: l.product,
                    variant: l.variant,
                    quantity: l.quantity.value(),
                    product_title: l.details.as_ref().map(|d| d.product_title.clone()),
                    variant_title: l.details.as_ref().and_then(|d| d.variant_title.clone()),
                    sku: l.details.as_ref().and_then(|d| d.sku.clone()),
                    handle: l.details.as_ref().and_then(|d| d.handle.clone()),
                    image: None,
                    available_quantity: l.sellable,
                    state: match &l.status {
                        LineStatus::Priced {
                            unit_price,
                            total,
                            tax,
                        } => LineState::Priced {
                            unit_price: unit_price.money(),
                            total: *total,
                            tax: tax.as_ref().map(TaxLineResponse::from),
                        },
                        LineStatus::Unavailable(reason) => LineState::Unavailable {
                            reason_code: reason.code(),
                            reason: reason.to_string(),
                        },
                    },
                })
                .collect(),
        }
    }
}

/// The response for `cart` with its lines' images: the assets loaded in one query and
/// their URLs made. Without file storage the lines show none.
pub async fn cart_response<P: HasAssets>(
    cart: &PricedCart,
    ports: &P,
    media: Option<&Media>,
) -> Result<CartResponse, ApiError> {
    let mut response = CartResponse::from(cart);
    let image = |l: &hypha_cart::PricedLine| l.details.as_ref().and_then(|d| d.image);
    let ids: Vec<AssetId> = cart.lines.iter().filter_map(image).collect();
    let Some(media) = media.filter(|_| !ids.is_empty()) else {
        return Ok(response);
    };
    let assets = ports.assets().get_many(&ids).await?;
    for (line, priced) in response.lines.iter_mut().zip(&cart.lines) {
        if let Some(asset) = image(priced).and_then(|id| assets.iter().find(|a| a.id() == id)) {
            line.image = Some(ProductMediaResponse::new(asset, media).await?);
        }
    }
    Ok(response)
}

/// Takes an `Idempotency-Key` header: a retry returns the cart the first request made.
#[utoipa::path(
    post,
    path = "/storefront/carts",
    params(("Idempotency-Key" = Option<String>, Header, description = "Makes the request safe to retry: a retry with the same key and body replays the first answer (marked `Idempotent-Replayed: true`) instead of repeating the effect. Same key with another body: `422 idempotency_key_reused`.")),
    request_body = CreateCartRequest,
    responses((status = 201, description = "Created", body = CartResponse, headers(("Idempotent-Replayed" = String, description = "`true` when this answer replays an earlier request with the same `Idempotency-Key`."))), (status = 429, description = "Over the rate limit for new carts per client address (`rate_limited`); see `Retry-After`", body = ApiErrorBody)),
    extensions(("x-hypha-scopes" = json!(["carts:write"]))),
)]
pub async fn create<S: Storage>(
    State(state): State<SharedState<S>>,
    ClientIp(ip): ClientIp,
    caller: Caller<Storefront>,
    req: KeyedJson<CreateCartRequest>,
) -> Result<Response, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    rate_limit::enforce(&state.limiter, Rule::CartCreateIp, &ip).await?;
    let req = req.parse()?;
    let input = CreateCart {
        country: req.body.country,
        idempotency: req.idempotency,
    };
    let created = create_cart(&ctx, &ports, &state.ids, &state.clock, input).await?;
    Ok(idempotent_response(
        StatusCode::CREATED,
        cart_response(&created.value, &ports, state.media.as_deref()).await?,
        created.replayed,
    ))
}

/// A cart, priced for its market and country.
#[utoipa::path(
    get,
    path = "/storefront/carts/{id}",
    params(("id" = CartId, Path)),
    responses((status = 200, description = "OK", body = CartResponse)),
    extensions(("x-hypha-scopes" = json!(["carts:read"]))),
)]
pub async fn fetch<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller<Storefront>,
    PathId(id): PathId<CartId>,
) -> Result<Json<CartResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let priced = hypha_checkout::view_cart(&ctx, &ports, id).await?;
    Ok(Json(
        cart_response(&priced, &ports, state.media.as_deref()).await?,
    ))
}

/// Takes an `Idempotency-Key` header: a retry returns the cart without adding again.
#[utoipa::path(
    post,
    path = "/storefront/carts/{id}/lines",
    params(("id" = CartId, Path), ("Idempotency-Key" = Option<String>, Header, description = "Makes the request safe to retry: a retry with the same key and body replays the first answer (marked `Idempotent-Replayed: true`) instead of repeating the effect. Same key with another body: `422 idempotency_key_reused`.")),
    request_body = AddLineRequest,
    responses((status = 200, description = "OK", body = CartResponse, headers(("Idempotent-Replayed" = String, description = "`true` when this answer replays an earlier request with the same `Idempotency-Key`.")))),
    extensions(("x-hypha-scopes" = json!(["carts:write"]))),
)]
pub async fn add<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller<Storefront>,
    PathId(id): PathId<CartId>,
    req: KeyedJson<AddLineRequest>,
) -> Result<Response, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = req.parse()?;
    let input = AddLine {
        cart: id,
        variant: req.body.variant,
        quantity: req.body.quantity,
        idempotency: req.idempotency,
    };
    let added = add_line(&ctx, &ports, &state.clock, input).await?;
    Ok(idempotent_response(
        StatusCode::OK,
        cart_response(&added.value, &ports, state.media.as_deref()).await?,
        added.replayed,
    ))
}

/// Sets the buyer's email; checkout finds or creates the customer from it.
#[utoipa::path(
    put,
    path = "/storefront/carts/{id}/email",
    params(("id" = CartId, Path)),
    request_body = SetEmailRequest,
    responses((status = 200, description = "OK", body = CartResponse)),
    extensions(("x-hypha-scopes" = json!(["carts:write"]))),
)]
pub async fn set_email<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller<Storefront>,
    PathId(id): PathId<CartId>,
    body: JsonBody<SetEmailRequest>,
) -> Result<Json<CartResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let priced = set_cart_email(&ctx, &ports, id, req.email).await?;
    Ok(Json(
        cart_response(&priced, &ports, state.media.as_deref()).await?,
    ))
}

/// Links a cart to a customer, or unlinks it with `null`.
#[utoipa::path(
    put,
    path = "/admin/carts/{id}/customer",
    params(("id" = CartId, Path)),
    request_body = SetCustomerRequest,
    responses((status = 200, description = "OK", body = CartResponse)),
    extensions(("x-hypha-scopes" = json!(["carts:write", "customers:read"]))),
)]
pub async fn set_customer<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<CartId>,
    body: JsonBody<SetCustomerRequest>,
) -> Result<Json<crate::redact::Redacted>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let priced = set_cart_customer(&ctx, &ports, id, req.customer).await?;
    let cart = cart_response(&priced, &ports, state.media.as_deref()).await?;
    // The admin surface: personal data per the caller (design note 0029, AP10).
    Ok(Json(crate::redact::one(
        &ctx,
        crate::redact::Family::Order,
        cart,
    )?))
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetQuantityRequest {
    /// At least 1; `DELETE` the line to take it out.
    quantity: u32,
}

/// Sets a line's quantity.
#[utoipa::path(
    put,
    path = "/storefront/carts/{id}/lines/{variant}",
    params(("id" = CartId, Path), ("variant" = VariantId, Path)),
    request_body = SetQuantityRequest,
    responses((status = 200, description = "OK", body = CartResponse)),
    extensions(("x-hypha-scopes" = json!(["carts:write"]))),
)]
pub async fn set_quantity<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller<Storefront>,
    PathId((id, variant)): PathId<(CartId, VariantId)>,
    body: JsonBody<SetQuantityRequest>,
) -> Result<Json<CartResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let input = SetLineQuantity {
        cart: id,
        variant,
        quantity: req.quantity,
    };
    let priced = set_line_quantity(&ctx, &ports, input)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("line_not_found"))?;
    Ok(Json(
        cart_response(&priced, &ports, state.media.as_deref()).await?,
    ))
}

/// Removes a line.
#[utoipa::path(
    delete,
    path = "/storefront/carts/{id}/lines/{variant}",
    params(("id" = CartId, Path), ("variant" = VariantId, Path)),
    responses((status = 200, description = "OK", body = CartResponse)),
    extensions(("x-hypha-scopes" = json!(["carts:write"]))),
)]
pub async fn remove_line<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller<Storefront>,
    PathId((id, variant)): PathId<(CartId, VariantId)>,
) -> Result<Json<CartResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let priced = remove_cart_line(&ctx, &ports, id, variant)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("line_not_found"))?;
    Ok(Json(
        cart_response(&priced, &ports, state.media.as_deref()).await?,
    ))
}

// ---------- shipping (design note 0015) ----------

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetShippingAddressRequest {
    /// In the cart's country; `null` clears it. A new address drops the chosen shipping.
    address: Option<PostalAddressRequest>,
}

/// Sets where the cart ships.
#[utoipa::path(
    put,
    path = "/storefront/carts/{id}/shipping-address",
    params(("id" = CartId, Path)),
    request_body = SetShippingAddressRequest,
    responses((status = 200, description = "OK", body = CartResponse)),
    extensions(("x-hypha-scopes" = json!(["carts:write"]))),
)]
pub async fn set_address<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller<Storefront>,
    PathId(id): PathId<CartId>,
    body: JsonBody<SetShippingAddressRequest>,
) -> Result<Json<CartResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let priced = set_shipping_address(&ctx, &ports, id, req.address.map(Into::into)).await?;
    Ok(Json(
        cart_response(&priced, &ports, state.media.as_deref()).await?,
    ))
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct ShippingOptionsResponse {
    groups: Vec<GroupOffersResponse>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct GroupOffersResponse {
    group: ShippingGroupId,
    /// What the buyer can pick, in the store's order, priced now.
    offers: Vec<OfferResponse>,
    /// Options of the zone not offered right now, and why: for diagnostics, never shown
    /// as choices.
    unavailable: Vec<UnavailableOptionResponse>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct OfferResponse {
    option: ShippingOptionId,
    code: String,
    name: String,
    /// Before tax if the market's prices exclude it.
    amount: Money,
    estimate: Option<DeliveryEstimateDto>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct UnavailableOptionResponse {
    option: ShippingOptionId,
    name: String,
    /// `no_price_in_currency`, `weight_unknown`, `provider_failed`, `service_not_quoted`,
    /// or `backup_not_needed` (a backup while a regular option is offered).
    reason: &'static str,
}

impl From<&GroupOffers> for GroupOffersResponse {
    fn from(g: &GroupOffers) -> Self {
        let mut offers = Vec::new();
        let mut unavailable = Vec::new();
        for outcome in &g.offers {
            match outcome {
                OfferOutcome::Offered(o) => offers.push(OfferResponse {
                    option: o.option,
                    code: o.code.clone(),
                    name: o.name.clone(),
                    amount: o.amount,
                    estimate: o.estimate.map(Into::into),
                }),
                OfferOutcome::NotOffered {
                    option,
                    name,
                    reason,
                } => unavailable.push(UnavailableOptionResponse {
                    option: *option,
                    name: name.clone(),
                    reason: reason.code(),
                }),
            }
        }
        Self {
            group: g.group,
            offers,
            unavailable,
        }
    }
}

/// What each shipping group can pick, priced now: built-in rates computed, rate
/// providers asked. Needs a shipping address and a zone for the cart's country.
#[utoipa::path(
    get,
    path = "/storefront/carts/{id}/shipping-options",
    params(("id" = CartId, Path)),
    responses((status = 200, description = "OK", body = ShippingOptionsResponse), (status = 429, description = "Over the rate limit for this cart (`rate_limited`); see `Retry-After`", body = ApiErrorBody)),
    extensions(("x-hypha-scopes" = json!(["carts:read"]))),
)]
pub async fn shipping_options<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller<Storefront>,
    PathId(id): PathId<CartId>,
) -> Result<Json<ShippingOptionsResponse>, ApiError> {
    // After the key: a stranger can't use up a cart's quota.
    let (ports, ctx) = caller.authenticate(&state).await?;
    rate_limit::enforce(&state.limiter, Rule::ShippingQuotesCart, &id.to_string()).await?;
    let rates = state.rates(&ports);
    let groups = shipping_offers(&ctx, &ports, &rates, id).await?;
    Ok(Json(ShippingOptionsResponse {
        groups: groups.iter().map(Into::into).collect(),
    }))
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SelectShippingRequest {
    /// One of the group's offers.
    option: ShippingOptionId,
}

/// Picks a group's shipping option. A provider's price is kept with the choice for 15
/// minutes; checkout asks again after that.
#[utoipa::path(
    put,
    path = "/storefront/carts/{id}/shipping/{group_id}",
    params(("id" = CartId, Path), ("group_id" = ShippingGroupId, Path)),
    request_body = SelectShippingRequest,
    responses((status = 200, description = "OK", body = CartResponse), (status = 429, description = "Over the rate limit for this cart (`rate_limited`); see `Retry-After`", body = ApiErrorBody)),
    extensions(("x-hypha-scopes" = json!(["carts:write"]))),
)]
pub async fn select<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller<Storefront>,
    PathId((id, group)): PathId<(CartId, ShippingGroupId)>,
    body: JsonBody<SelectShippingRequest>,
) -> Result<Json<CartResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    rate_limit::enforce(&state.limiter, Rule::ShippingQuotesCart, &id.to_string()).await?;
    let req = body.parse()?;
    let rates = state.rates(&ports);
    let priced = select_shipping(&ctx, &ports, &rates, &state.clock, id, group, req.option).await?;
    Ok(Json(
        cart_response(&priced, &ports, state.media.as_deref()).await?,
    ))
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct CartPaymentMethodsResponse {
    /// Enabled, offered in the cart's market and channel, in the order to show them.
    items: Vec<CartPaymentMethodResponse>,
}

/// How the buyer can pay for the cart (design note 0027, §4).
#[utoipa::path(
    get,
    path = "/storefront/carts/{id}/payment-methods",
    params(("id" = CartId, Path)),
    responses((status = 200, description = "OK", body = CartPaymentMethodsResponse)),
    extensions(("x-hypha-scopes" = json!(["carts:read"]))),
)]
pub async fn payment_methods<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller<Storefront>,
    PathId(id): PathId<CartId>,
) -> Result<Json<CartPaymentMethodsResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let providers = state.storage.payment_providers();
    let methods = hypha_checkout::cart_payment_methods(&ctx, &ports, providers, id).await?;
    Ok(Json(CartPaymentMethodsResponse {
        items: methods.iter().map(CartPaymentMethodResponse::new).collect(),
    }))
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetPaymentMethodRequest {
    /// One of the cart's `payment-methods`; `null` clears the choice. Required, `null`
    /// included: `{}` is `422 invalid_body`, not a quiet clear.
    #[serde(deserialize_with = "crate::patch::required")]
    #[schema(required = true)]
    payment_method: Option<PaymentMethodId>,
}

/// Chooses how the buyer pays: a method offered to the cart (422
/// `payment_method_unavailable` otherwise, `payment_method_not_found` for an unknown
/// one). Checkout checks it again.
#[utoipa::path(
    put,
    path = "/storefront/carts/{id}/payment-method",
    params(("id" = CartId, Path)),
    request_body = SetPaymentMethodRequest,
    responses((status = 200, description = "OK", body = CartResponse)),
    extensions(("x-hypha-scopes" = json!(["carts:write"]))),
)]
pub async fn set_payment_method<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller<Storefront>,
    PathId(id): PathId<CartId>,
    body: JsonBody<SetPaymentMethodRequest>,
) -> Result<Json<CartResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let providers = state.storage.payment_providers();
    let priced =
        hypha_checkout::set_payment_method(&ctx, &ports, providers, id, req.payment_method)
            .await
            // The method arrives in the body: invalid input, not the cart's state.
            .map_err(|e| ApiError::from(e).unprocessable_if("payment_method_unavailable"))?;
    Ok(Json(
        cart_response(&priced, &ports, state.media.as_deref()).await?,
    ))
}
