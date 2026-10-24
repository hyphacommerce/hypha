use axum::{Json, extract::State, http::StatusCode, response::Response};
use hypha_cart::{
    AddLine, CreateCart, GroupOffers, LineStatus, PricedCart, PricedShippingGroup, SetLineQuantity,
    add_discount_code, add_line, create_cart, remove_cart_line, remove_discount_code,
    select_shipping, set_cart_customer, set_cart_email, set_line_quantity, set_shipping_address,
    shipping_offers,
};
use hypha_core::{
    AssetId, CartId, CountryCode, Currency, CustomerId, DiscountId, Email, ErrorCode, MarketId,
    Money, OrderId, PaymentMethodId, ProductId, ShippingGroupId, ShippingOptionId, TaxClassId,
    TaxRate, VariantId,
};
use hypha_customers::{HasCustomers, get_customer};
use hypha_media::{AssetRepo, HasAssets};
use hypha_shipping::OfferOutcome;
use hypha_tax::TaxLine;
use serde::{Deserialize, Serialize};

use super::{
    address::{PostalAddressRequest, PostalAddressResponse},
    discounts::{DiscountClassDto, DiscountTargetDto, DiscountValueDto},
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
    /// Personal fields left out for this caller: an API key or app without the matching
    /// `customer_data:*` scope. They are absent from the object then, whatever the schema
    /// says. Never sent to staff or storefronts.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    redacted: Option<Vec<String>>,
    customer: Option<CustomerId>,
    /// The order this cart was checked out as; `null` while it can still change.
    order: Option<OrderId>,
    /// Whether line prices include tax (set on the market).
    taxes_included: bool,
    /// Sum of the available lines' totals, after discounts. All totals count available
    /// lines only.
    subtotal: Money,
    /// The chosen shipping, priced now, after shipping discounts; zero until chosen. Part
    /// of `total`.
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
    /// Whether the buyer ticked "Email me with news and offers" (`PUT …/marketing`):
    /// checkout records the consent then.
    marketing_opt_in: bool,
    /// The discounts applied, one per reward: product ones first, then order, then
    /// shipping. Lines and shipping name them by their index here.
    discounts: Vec<CartDiscountResponse>,
    /// Everything they took off: already out of `subtotal` and `shipping_total`.
    discount_total: Money,
    /// Each code the cart holds, in the order entered, with where it stands. A code that
    /// doesn't apply now is kept: it applies once the cart qualifies.
    discount_codes: Vec<CartDiscountCodeResponse>,
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
    /// The option's price, before shipping discounts.
    amount: Money,
    /// `null` when the cart's country has no tax rates.
    tax: Option<TaxLineResponse>,
    estimate: Option<DeliveryEstimateDto>,
    /// RFC 3339: for a provider's rate, when checkout asks the provider again (and may
    /// answer `total_changed`). `null` for built-in rates, which are always current.
    quote_expires_at: Option<String>,
    /// What shipping discounts took off it.
    discounts: Vec<DiscountShareResponse>,
    /// What the buyer pays for it: `amount` less the discounts. Tax is on this.
    total: Money,
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
                discounts: s
                    .discounts
                    .iter()
                    .map(DiscountShareResponse::from)
                    .collect(),
                total: s.total,
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
        /// What the buyer pays for the line: `subtotal` less the discounts. Tax is on this.
        total: Money,
        /// `null` when the cart's country has no tax rates.
        tax: Option<TaxLineResponse>,
        /// What `unit_price` is shown against, struck through: only ever above it; `null`
        /// for none.
        compare_at_price: Option<Money>,
        /// `unit_price` × `quantity`: before discounts.
        subtotal: Money,
        /// What discounts took off the line.
        discounts: Vec<DiscountShareResponse>,
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
            discounts: cart
                .discounts
                .iter()
                .map(CartDiscountResponse::from)
                .collect(),
            discount_total: cart.discount_total,
            discount_codes: cart
                .discount_codes
                .iter()
                .map(CartDiscountCodeResponse::from)
                .collect(),
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
            marketing_opt_in: cart.marketing_opt_in,
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
                            compare_at,
                            subtotal,
                            discounts,
                            total,
                            tax,
                            ..
                        } => LineState::Priced {
                            unit_price: unit_price.money(),
                            compare_at_price: compare_at.map(|p| p.money()),
                            subtotal: *subtotal,
                            discounts: discounts.iter().map(DiscountShareResponse::from).collect(),
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

/// A discount applied to the cart: one reward of it.
#[derive(Serialize, utoipa::ToSchema)]
pub struct CartDiscountResponse {
    discount: DiscountId,
    /// The code it came by, normalised; `null` for an automatic discount.
    code: Option<String>,
    /// The discount's title, shown to buyers.
    title: String,
    /// One sentence from its data, amounts in the cart's market.
    summary: String,
    class: DiscountClassDto,
    value: DiscountValueDto,
    target: DiscountTargetDto,
    /// What it took off in all.
    amount: Money,
}

impl From<&hypha_cart::discounts::Application> for CartDiscountResponse {
    fn from(a: &hypha_cart::discounts::Application) -> Self {
        Self {
            discount: a.discount,
            code: a.code.clone(),
            title: a.title.clone(),
            summary: a.summary.clone(),
            class: a.class.into(),
            value: (&a.value).into(),
            target: (&a.target).into(),
            amount: a.amount,
        }
    }
}

/// Part of a discount on a line or a shipping group.
#[derive(Serialize, utoipa::ToSchema)]
pub struct DiscountShareResponse {
    /// Its index in the cart's (or order's) `discounts`.
    application: usize,
    amount: Money,
    /// The units it covered, for a buy X get Y discount; absent otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    units: Option<u32>,
}

impl From<&hypha_cart::discounts::Share> for DiscountShareResponse {
    fn from(s: &hypha_cart::discounts::Share) -> Self {
        Self {
            application: s.application,
            amount: s.amount,
            units: s.units,
        }
    }
}

/// Where a code the cart holds stands, for a UCP-style message pointing at
/// `$.discount_codes[i]`.
#[derive(Serialize, utoipa::ToSchema)]
pub struct CartDiscountCodeResponse {
    /// Normalised.
    code: String,
    /// `applied`; `not_eligible` (see `condition`); `minimum_not_met` (see `needed`);
    /// `no_matching_items`; `blocked_by_other` (a better combination left it out, see
    /// `discount`); `limit_reached`; `not_available_in_market`; `expired` (its end date
    /// passed); `invalid` (the code or its discount is gone, deactivated, a draft or not
    /// started). More may be added: treat an unknown one as not applied.
    state: &'static str,
    /// For `not_eligible`: `markets`, `channels`, `customer_groups`, `customers` or
    /// `signed_in`.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    condition: Option<&'static str>,
    /// For `minimum_not_met`: what's missing.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    needed: Option<NeededResponse>,
    /// For `blocked_by_other`: a discount applied instead.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    discount: Option<DiscountId>,
}

/// What's missing for a minimum: one of the two.
#[derive(Serialize, utoipa::ToSchema)]
pub struct NeededResponse {
    /// This much more of the qualifying items.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    subtotal: Option<Money>,
    /// This many more qualifying units.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    quantity: Option<u32>,
}

impl From<&hypha_cart::discounts::CodeOutcome> for CartDiscountCodeResponse {
    fn from(c: &hypha_cart::discounts::CodeOutcome) -> Self {
        use hypha_cart::discounts::{CodeState, Needed};
        let mut response = Self {
            code: c.code.clone(),
            state: c.state.name(),
            condition: None,
            needed: None,
            discount: None,
        };
        match c.state {
            CodeState::NotEligible { condition } => response.condition = Some(condition),
            CodeState::MinimumNotMet { needed } => {
                response.needed = Some(match needed {
                    Needed::Amount(m) => NeededResponse {
                        subtotal: Some(m),
                        quantity: None,
                    },
                    Needed::Quantity(n) => NeededResponse {
                        subtotal: None,
                        quantity: Some(n),
                    },
                })
            }
            CodeState::BlockedByOther { discount } => response.discount = Some(discount),
            CodeState::Applied
            | CodeState::NoMatchingItems
            | CodeState::LimitReached
            | CodeState::NotAvailableInMarket
            | CodeState::Expired
            | CodeState::Invalid => {}
        }
        response
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AddDiscountCodeRequest {
    /// As the buyer typed it: it's trimmed and compared ignoring case.
    code: String,
}

/// Enters a discount code. A code that isn't active and live (or isn't one) is `422
/// discount_code_invalid`, the same for each, and isn't kept; a live code that doesn't
/// apply yet is kept with its state, so it applies once the cart qualifies. At most 5
/// (`409 too_many_discount_codes`). Rate limited per client address and per cart.
#[utoipa::path(
    post,
    path = "/storefront/carts/{id}/discount-codes",
    params(("id" = CartId, Path)),
    request_body = AddDiscountCodeRequest,
    responses((status = 200, description = "OK", body = CartResponse), (status = 429, description = "Over the rate limit for codes per client address or per cart (`rate_limited`); see `Retry-After`", body = ApiErrorBody)),
    extensions(("x-hypha-scopes" = json!(["carts:write"]))),
)]
pub async fn add_code<S: Storage>(
    State(state): State<SharedState<S>>,
    ClientIp(ip): ClientIp,
    caller: Caller<Storefront>,
    PathId(id): PathId<CartId>,
    body: JsonBody<AddDiscountCodeRequest>,
) -> Result<Json<CartResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    rate_limit::enforce(&state.limiter, Rule::DiscountCodeIp, &ip).await?;
    rate_limit::enforce(&state.limiter, Rule::DiscountCodeCart, &id.to_string()).await?;
    let req = body.parse()?;
    let priced = add_discount_code(&ctx, &ports, id, &req.code).await?;
    Ok(Json(
        cart_response(&priced, &ports, state.media.as_deref()).await?,
    ))
}

/// Lets go of a discount code (as entered, or normalised).
#[utoipa::path(
    delete,
    path = "/storefront/carts/{id}/discount-codes/{code}",
    params(("id" = CartId, Path), ("code" = String, Path, example = "WELCOME10", description = "The code as entered, or normalised.")),
    responses((status = 200, description = "OK", body = CartResponse)),
    extensions(("x-hypha-scopes" = json!(["carts:write"]))),
)]
pub async fn remove_code<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller<Storefront>,
    PathId((id, code)): PathId<(CartId, String)>,
) -> Result<Json<CartResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let priced = remove_discount_code(&ctx, &ports, id, &code).await?;
    Ok(Json(
        cart_response(&priced, &ports, state.media.as_deref()).await?,
    ))
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
    // A signed-in shopper's cart is theirs from the start.
    let customer = match hypha_customer_accounts::own(&ctx) {
        Ok(owned) => {
            let me = get_customer(&owned.ctx, ports.customers(), owned.customer).await?;
            Some((me.id(), me.email().clone()))
        }
        Err(_) => None,
    };
    let input = CreateCart {
        country: req.body.country,
        idempotency: req.idempotency,
        customer,
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

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetMarketingRequest {
    /// Whether the buyer wants news and offers by email.
    opt_in: bool,
}

/// Ticks or unticks "Email me with news and offers". Checkout records the consent when it's
/// ticked; unticked never unsubscribes anyone.
#[utoipa::path(
    put,
    path = "/storefront/carts/{id}/marketing",
    params(("id" = CartId, Path)),
    request_body = SetMarketingRequest,
    responses((status = 200, description = "OK", body = CartResponse)),
    extensions(("x-hypha-scopes" = json!(["carts:write"]))),
)]
pub async fn set_marketing<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller<Storefront>,
    PathId(id): PathId<CartId>,
    body: JsonBody<SetMarketingRequest>,
) -> Result<Json<CartResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let priced = hypha_cart::set_cart_marketing(&ctx, &ports, id, req.opt_in).await?;
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
    // The admin surface: personal data per the caller.
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

// ---------- shipping ----------

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
    /// Before tax if the market's prices exclude it, and before shipping discounts.
    amount: Money,
    estimate: Option<DeliveryEstimateDto>,
    /// What it comes to after the shipping discounts the cart has now (0: free).
    amount_after_discounts: Money,
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
        for (i, outcome) in g.offers.iter().enumerate() {
            match outcome {
                OfferOutcome::Offered(o) => offers.push(OfferResponse {
                    option: o.option,
                    code: o.code.clone(),
                    name: o.name.clone(),
                    amount: o.amount,
                    amount_after_discounts: g
                        .after_discounts
                        .get(i)
                        .copied()
                        .flatten()
                        .unwrap_or(o.amount),
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
    let rates = state.cart_rates(&ports, id);
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
    let rates = state.cart_rates(&ports, id);
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

/// How the buyer can pay for the cart.
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
    let providers = state.payments(&ports);
    let methods = hypha_checkout::cart_payment_methods(&ctx, &ports, &providers, id).await?;
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
    let providers = state.payments(&ports);
    let priced =
        hypha_checkout::set_payment_method(&ctx, &ports, &providers, id, req.payment_method)
            .await
            // The method arrives in the body: invalid input, not the cart's state.
            .map_err(|e| ApiError::from(e).unprocessable_if("payment_method_unavailable"))?;
    Ok(Json(
        cart_response(&priced, &ports, state.media.as_deref()).await?,
    ))
}
