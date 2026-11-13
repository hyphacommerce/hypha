use axum::{Json, extract::State, http::StatusCode, response::Response};
use hypha_cart::{
    AddLine, CartStatus, ContentLanguages, CreateCart, GroupOffers, LineStatus, PricedCart,
    PricedShippingGroup, PricingPorts, SetLineQuantity, add_discount_code, add_line, create_cart,
    remove_cart_line, remove_discount_code, select_shipping, set_cart_customer, set_cart_email,
    set_cart_language, set_line_quantity, set_shipping_address, shipping_offers,
};
use hypha_checkout::CartChecks;
use hypha_checkout_rules::HasCheckoutRules;
use hypha_core::{
    AssetId, CartId, CheckoutRuleId, CountryCode, Currency, CustomerId, DiscountId, Email,
    ErrorCode, Language, MarketId, Message, MessageSource, Money, OrderId, PaymentMethodId,
    ProductId, Severity, ShippingGroupId, ShippingOptionId, TaxClassId, TaxRate, VariantId,
};
use hypha_customers::{HasCustomers, get_customer};
use hypha_markets::{HasMarkets, MarketRepo};
use hypha_media::{AssetRepo, HasAssets};
use hypha_orders::{HasOrders, OrderRepo};
use hypha_shipping::OfferOutcome;
use hypha_tax::TaxLine;
use hypha_translations::{HasTranslations, Overlay, ResourceKind, ResourceRef, fields as keys};
use serde::{Deserialize, Serialize};

use super::{
    address::{PostalAddressRequest, PostalAddressResponse, ShippingAddressCheckResponse},
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
    language::{self, Localized, ServedLanguage},
    media::Media,
    rate_limit::{self, Rule},
    state::{SharedState, Storage},
};

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateCartRequest {
    /// Where the cart ships to; picks the market and the tax rates.
    country: String,
    /// The language the shopper reads the cart in (a BCP 47 tag), out of those the
    /// market offers (`/storefront/countries`): one not offered is tried without its
    /// region (`de` for `de-AT`), else the market's default is kept. Absent, the cart
    /// follows the market's default. The order is placed in it.
    #[serde(default)]
    language: Option<String>,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetLanguageRequest {
    /// A BCP 47 tag, held to the cart's market like `language` on create: what it
    /// resolves to is kept. `null` goes back to following the market's default.
    /// Required, `null` included.
    #[serde(deserialize_with = "crate::patch::required")]
    #[schema(required = true)]
    language: Option<String>,
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
    /// What the store's address verification app said about the shipping address (asked
    /// when it was set); `null` without an address or a verification app.
    shipping_address_check: Option<ShippingAddressCheckResponse>,
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
    /// What the shopper should know about the cart, each under the field its `path` names:
    /// errors stop checkout and starting its payment (`409 checkout_blocked`), warnings
    /// never do. Empty once the cart is checked out.
    messages: Vec<CartMessageResponse>,
    /// The language the shopper chose for the cart (`PUT …/language`), as kept: `null`
    /// follows the market's default. A storefront's answer is in the language the cart
    /// reads in now, which `Content-Language` names; the admin's is in the store's.
    language: Option<Language>,
}

/// Something about the cart for the shopper to see or fix.
#[derive(Serialize, utoipa::ToSchema)]
pub struct CartMessageResponse {
    severity: SeverityDto,
    /// snake_case, for code. Built-in codes (`house_number_missing`) are a contract like
    /// error codes; a checkout rule's is the merchant's, a validation app's the app's.
    code: String,
    /// An RFC 9535 JSONPath into this response, from a closed set: `$` (the cart),
    /// `$.email`, `$.shipping_address`, `$.shipping_address.<field>`,
    /// `$.shipping_groups[<i>]` (the group and its chosen option) or `$.lines[<i>]`. Show
    /// the message by that field; one a client doesn't place goes at the top.
    #[schema(example = "$.shipping_address.line1")]
    path: String,
    /// Plain text for the shopper.
    message: String,
    source: MessageSourceDto,
}

/// Whether a message stops checkout: `error` does, `warning` never.
#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SeverityDto {
    Error,
    Warning,
}

/// Who said it: the address's format, the store's verification app, one of the merchant's
/// checkout rules, or a validation app.
#[derive(Serialize, utoipa::ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MessageSourceDto {
    Format,
    Verification,
    Rule {
        rule: CheckoutRuleId,
    },
    App {
        /// The app's key.
        app: String,
    },
}

impl From<&Message> for CartMessageResponse {
    fn from(m: &Message) -> Self {
        Self {
            severity: match m.severity() {
                Severity::Error => SeverityDto::Error,
                Severity::Warning => SeverityDto::Warning,
            },
            code: m.code().to_string(),
            path: m.path().to_string(),
            message: m.message().to_string(),
            source: match m.source() {
                MessageSource::Format => MessageSourceDto::Format,
                MessageSource::Verification => MessageSourceDto::Verification,
                MessageSource::Rule { rule } => MessageSourceDto::Rule { rule: *rule },
                MessageSource::App { app } => MessageSourceDto::App {
                    app: app.to_string(),
                },
            },
        }
    }
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
            // Filled by `cart_response`, which reads the store's checkout rules.
            messages: Vec::new(),
            language: cart.language.clone(),
            shipping_address_check: None,
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
    /// One sentence from its data, naming what it applies to as a shopper may see it,
    /// amounts in the cart's market.
    summary: String,
    class: DiscountClassDto,
    value: DiscountValueDto,
    target: DiscountTargetDto,
    /// What it took off in all.
    amount: Money,
}

impl From<&hypha_cart::promotions::Application> for CartDiscountResponse {
    /// Value and target come as the discounts' JSON; JSON in another shape reads as
    /// nothing off the subtotal (it can't be, short of a bug), so the cart still reads.
    fn from(a: &hypha_cart::promotions::Application) -> Self {
        use hypha_cart::promotions::Class;
        use hypha_discounts::{
            Target, Value,
            json::{target_from_json, value_from_json},
        };
        let value =
            value_from_json(&a.value).unwrap_or(Value::Percent(hypha_core::BasisPoints::ZERO));
        let target = target_from_json(&a.target).unwrap_or(Target::Subtotal);
        Self {
            discount: a.discount,
            code: a.code.clone(),
            title: a.title.clone(),
            summary: a.summary.clone(),
            class: match a.class {
                Class::Product => DiscountClassDto::Product,
                Class::Order => DiscountClassDto::Order,
                Class::Shipping => DiscountClassDto::Shipping,
            },
            value: (&value).into(),
            target: (&target).into(),
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

impl From<&hypha_cart::promotions::Share> for DiscountShareResponse {
    fn from(s: &hypha_cart::promotions::Share) -> Self {
        Self {
            application: s.application,
            amount: s.amount,
            units: s.units,
        }
    }
}

impl From<&hypha_orders::DiscountShare> for DiscountShareResponse {
    fn from(s: &hypha_orders::DiscountShare) -> Self {
        Self {
            application: usize::from(s.application),
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
    /// `discount`); `no_saving` (with it the cart would cost no less, say by falling under
    /// free shipping); `limit_reached`; `not_available_in_market`; `expired` (its end date
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

impl From<&hypha_cart::promotions::CodeOutcome> for CartDiscountCodeResponse {
    fn from(c: &hypha_cart::promotions::CodeOutcome) -> Self {
        use hypha_cart::promotions::{CodeState, Needed};
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
                });
            }
            CodeState::BlockedByOther { discount } => response.discount = Some(discount),
            CodeState::Applied
            | CodeState::NoMatchingItems
            | CodeState::NoSaving
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
    responses((status = 200, description = "OK", body = CartResponse, headers(("Content-Language" = String, description = "The language the response is in."))), (status = 429, description = "Over the rate limit for codes per client address or per cart (`rate_limited`); see `Retry-After`", body = ApiErrorBody)),
    extensions(("x-hypha-scopes" = json!(["carts:write"]))),
)]
pub async fn add_code<S: Storage>(
    State(state): State<SharedState<S>>,
    ClientIp(ip): ClientIp,
    caller: Caller<Storefront>,
    PathId(id): PathId<CartId>,
    body: JsonBody<AddDiscountCodeRequest>,
) -> Result<Localized<CartResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    rate_limit::enforce(&state.limiter, Rule::DiscountCodeIp, &ip).await?;
    rate_limit::enforce(&state.limiter, Rule::DiscountCodeCart, &id.to_string()).await?;
    let req = body.parse()?;
    let priced = add_discount_code(&ctx, &ports, id, &req.code).await?;
    storefront_cart(&state, &ports, &priced, &state.cart_checks(&ports)).await
}

/// Lets go of a discount code (as entered, or normalised).
#[utoipa::path(
    delete,
    path = "/storefront/carts/{id}/discount-codes/{code}",
    params(("id" = CartId, Path), ("code" = String, Path, example = "WELCOME10", description = "The code as entered, or normalised.")),
    responses((status = 200, description = "OK", body = CartResponse, headers(("Content-Language" = String, description = "The language the response is in.")))),
    extensions(("x-hypha-scopes" = json!(["carts:write"]))),
)]
pub async fn remove_code<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller<Storefront>,
    PathId((id, code)): PathId<(CartId, String)>,
) -> Result<Localized<CartResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let priced = remove_discount_code(&ctx, &ports, id, &code).await?;
    storefront_cart(&state, &ports, &priced, &state.cart_checks(&ports)).await
}

/// The response for `cart` with its messages and its lines' images: the assets loaded in
/// one query and their URLs made. Without file storage the lines show none. In `served`'s
/// language (a storefront's), its names (lines' titles, image alt texts, the chosen
/// shipping option, discounts' titles, checkout rules' messages) come from one overlay;
/// without it (the admin's), as stored.
pub async fn cart_response<P: HasAssets + PricingPorts + HasCheckoutRules + HasTranslations>(
    cart: &PricedCart,
    ports: &P,
    checks: &impl CartChecks,
    languages: &ContentLanguages,
    media: Option<&Media>,
    served: Option<&ServedLanguage>,
) -> Result<CartResponse, ApiError> {
    let mut response = CartResponse::from(cart);
    response.shipping_address_check = cart
        .address_check
        .as_ref()
        .filter(|_| cart.shipping_address.is_some())
        .map(ShippingAddressCheckResponse::new)
        .transpose()?;
    let messages = hypha_checkout::cart_messages(ports, checks, languages, cart).await?;
    let image = |l: &hypha_cart::PricedLine| l.details.as_ref().and_then(|d| d.image);
    let ids: Vec<AssetId> = cart.lines.iter().filter_map(image).collect();
    let media = media.filter(|_| !ids.is_empty());
    // A checked-out cart reads its order's names, already in its language.
    let overlay = match served.filter(|s| !s.is_source() && cart.order.is_none()) {
        Some(served) => {
            let resources = cart_resources(cart, &messages, media.map(|_| ids.as_slice()));
            served.overlay(ports, &resources).await?
        }
        None => Overlay::none(),
    };
    translate_cart(&mut response, cart, &overlay);
    response.messages = messages
        .iter()
        .map(|m| {
            let mut shown = CartMessageResponse::from(m);
            if let MessageSource::Rule { rule } = m.source() {
                let of_rule = ResourceRef::new(ResourceKind::CheckoutRule, rule.as_uuid());
                if let Some(text) = overlay.text(of_rule, keys::checkout_rule::MESSAGE) {
                    shown.message = text.to_string();
                }
            }
            shown
        })
        .collect();
    let Some(media) = media else {
        return Ok(response);
    };
    let assets = ports.assets().get_many(&ids).await?;
    for (line, priced) in response.lines.iter_mut().zip(&cart.lines) {
        if let Some(asset) = image(priced).and_then(|id| assets.iter().find(|a| a.id() == id)) {
            line.image = Some(ProductMediaResponse::translated(asset, media, &overlay).await?);
        }
    }
    Ok(response)
}

/// Every translatable resource a cart's response names: its products, the chosen
/// options, its discounts, the rules among its messages and (`images`) its lines' images.
fn cart_resources(
    cart: &PricedCart,
    messages: &[Message],
    images: Option<&[AssetId]>,
) -> Vec<ResourceRef> {
    let mut resources: Vec<ResourceRef> = Vec::new();
    let mut add = |r: ResourceRef| {
        if !resources.contains(&r) {
            resources.push(r);
        }
    };
    for line in &cart.lines {
        add(ResourceRef::new(
            ResourceKind::Product,
            line.product.as_uuid(),
        ));
    }
    for group in &cart.shipping_groups {
        if let Some(selected) = &group.selected {
            add(ResourceRef::new(
                ResourceKind::ShippingOption,
                selected.option.as_uuid(),
            ));
        }
    }
    for applied in &cart.discounts {
        add(ResourceRef::new(
            ResourceKind::Discount,
            applied.discount.as_uuid(),
        ));
    }
    for message in messages {
        if let MessageSource::Rule { rule } = message.source() {
            add(ResourceRef::new(ResourceKind::CheckoutRule, rule.as_uuid()));
        }
    }
    for asset in images.unwrap_or_default() {
        add(ResourceRef::new(ResourceKind::Asset, asset.as_uuid()));
    }
    resources
}

/// `response`'s names as `overlay` translates them: lines' titles (a variant's joined
/// from its values' labels), the chosen options' names and discounts' titles.
fn translate_cart(response: &mut CartResponse, cart: &PricedCart, overlay: &Overlay) {
    for (line, priced) in response.lines.iter_mut().zip(&cart.lines) {
        let product = ResourceRef::new(ResourceKind::Product, priced.product.as_uuid());
        let Some(details) = priced
            .details
            .as_ref()
            .filter(|_| overlay.translates(product))
        else {
            continue;
        };
        line.product_title = Some(
            overlay
                .text_or(product, keys::product::TITLE, &details.product_title)
                .to_string(),
        );
        line.variant_title = details.variant_title_with(|value| {
            overlay.text(product, &keys::product::option_value_label(value))
        });
    }
    for (group, priced) in response
        .shipping_groups
        .iter_mut()
        .zip(&cart.shipping_groups)
    {
        if let (Some(shown), Some(selected)) = (group.selected.as_mut(), priced.selected.as_ref()) {
            let option = ResourceRef::new(ResourceKind::ShippingOption, selected.option.as_uuid());
            if let Some(name) = overlay.text(option, keys::shipping_option::NAME) {
                shown.name = name.to_string();
            }
        }
    }
    for (shown, applied) in response.discounts.iter_mut().zip(&cart.discounts) {
        let discount = ResourceRef::new(ResourceKind::Discount, applied.discount.as_uuid());
        if let Some(title) = overlay.text(discount, keys::discount::TITLE) {
            shown.title = title.to_string();
        }
    }
}

/// The storefront's answer about `cart`: in the language it reads in now (the cart's
/// choice held to what its market offers, the store read as the system), named by
/// `Content-Language`.
pub(crate) async fn storefront_cart<S: Storage>(
    state: &crate::state::AppState<S>,
    ports: &S::Ports<'_>,
    cart: &PricedCart,
    checks: &impl CartChecks,
) -> Result<Localized<CartResponse>, ApiError> {
    let store = super::stores::store_as_system(state, ports).await?;
    let (served, response) = cart_answer(state, ports, cart, checks, &store).await?;
    Ok(served.respond(response))
}

/// The storefront's answer about `cart` and the language it's in, the store read already.
async fn cart_answer<S: Storage>(
    state: &crate::state::AppState<S>,
    ports: &S::Ports<'_>,
    cart: &PricedCart,
    checks: &impl CartChecks,
    store: &hypha_stores::Store,
) -> Result<(ServedLanguage, CartResponse), ApiError> {
    let languages = crate::language::content_languages(store);
    let served = ServedLanguage::of_cart(cart, store);
    let response = cart_response(
        cart,
        ports,
        checks,
        &languages,
        state.media.as_deref(),
        Some(&served),
    )
    .await?;
    Ok((served, response))
}

/// A keyed write's answer about a cart: `Idempotent-Replayed` when it replays one, and
/// `Content-Language`.
fn keyed_cart(
    status: StatusCode,
    (served, response): (ServedLanguage, CartResponse),
    replayed: bool,
) -> Response {
    let mut answer = idempotent_response(status, response, replayed);
    language::name_language(&mut answer, served.language());
    answer
}

/// Takes an `Idempotency-Key` header: a retry returns the cart the first request made.
#[utoipa::path(
    post,
    path = "/storefront/carts",
    params(("Idempotency-Key" = Option<String>, Header, description = "Makes the request safe to retry: a retry with the same key and body replays the first answer (marked `Idempotent-Replayed: true`) instead of repeating the effect. Same key with another body: `422 idempotency_key_reused`.")),
    request_body = CreateCartRequest,
    responses((status = 201, description = "Created", body = CartResponse, headers(("Idempotent-Replayed" = String, description = "`true` when this answer replays an earlier request with the same `Idempotency-Key`."), ("Content-Language" = String, description = "The language the response is in."))), (status = 429, description = "Over the rate limit for new carts per client address (`rate_limited`); see `Retry-After`", body = ApiErrorBody)),
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
    // Held to what the country's market offers; a country no market sells to is the
    // command's to refuse.
    let store = super::stores::store_as_system(&state, &ports).await?;
    let language = match req.body.language.as_deref() {
        Some(asked) => {
            let market = match CountryCode::new(&req.body.country) {
                Ok(country) => ports.markets().get_by_country(country).await?,
                Err(_) => None,
            };
            language::chosen(Some(asked), &store, market.as_ref())
        }
        None => None,
    };
    let input = CreateCart {
        country: req.body.country,
        idempotency: req.idempotency,
        customer,
        language,
    };
    let created = create_cart(&ctx, &ports, &state.ids, &state.clock, input).await?;
    let checks = state.cart_checks(&ports);
    let answer = cart_answer(&state, &ports, &created.value, &checks, &store).await?;
    Ok(keyed_cart(StatusCode::CREATED, answer, created.replayed))
}

/// A cart, priced for its market and country.
#[utoipa::path(
    get,
    path = "/storefront/carts/{id}",
    params(("id" = CartId, Path)),
    responses((status = 200, description = "OK", body = CartResponse, headers(("Content-Language" = String, description = "The language the response is in.")))),
    extensions(("x-hypha-scopes" = json!(["carts:read"]))),
)]
pub async fn fetch<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller<Storefront>,
    PathId(id): PathId<CartId>,
) -> Result<Localized<CartResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let priced = hypha_checkout::view_cart(&ctx, &ports, id).await?;
    storefront_cart(&state, &ports, &priced, &state.cart_checks(&ports)).await
}

/// Takes an `Idempotency-Key` header: a retry returns the cart without adding again.
#[utoipa::path(
    post,
    path = "/storefront/carts/{id}/lines",
    params(("id" = CartId, Path), ("Idempotency-Key" = Option<String>, Header, description = "Makes the request safe to retry: a retry with the same key and body replays the first answer (marked `Idempotent-Replayed: true`) instead of repeating the effect. Same key with another body: `422 idempotency_key_reused`.")),
    request_body = AddLineRequest,
    responses((status = 200, description = "OK", body = CartResponse, headers(("Idempotent-Replayed" = String, description = "`true` when this answer replays an earlier request with the same `Idempotency-Key`."), ("Content-Language" = String, description = "The language the response is in.")))),
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
    let checks = state.cart_checks(&ports);
    let store = super::stores::store_as_system(&state, &ports).await?;
    let answer = cart_answer(&state, &ports, &added.value, &checks, &store).await?;
    Ok(keyed_cart(StatusCode::OK, answer, added.replayed))
}

/// Sets the buyer's email; checkout finds or creates the customer from it.
#[utoipa::path(
    put,
    path = "/storefront/carts/{id}/email",
    params(("id" = CartId, Path)),
    request_body = SetEmailRequest,
    responses((status = 200, description = "OK", body = CartResponse, headers(("Content-Language" = String, description = "The language the response is in.")))),
    extensions(("x-hypha-scopes" = json!(["carts:write"]))),
)]
pub async fn set_email<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller<Storefront>,
    PathId(id): PathId<CartId>,
    body: JsonBody<SetEmailRequest>,
) -> Result<Localized<CartResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let priced = set_cart_email(&ctx, &ports, id, req.email).await?;
    storefront_cart(&state, &ports, &priced, &state.cart_checks(&ports)).await
}

/// Sets the language the shopper reads the cart in, held to what its market offers (a tag
/// not offered tries its language, then the market's default, which is what's kept);
/// `null` follows the market's default. Answers the cart in it.
#[utoipa::path(
    put,
    path = "/storefront/carts/{id}/language",
    params(("id" = CartId, Path)),
    request_body = SetLanguageRequest,
    responses((status = 200, description = "OK", body = CartResponse, headers(("Content-Language" = String, description = "The language the response is in.")))),
    extensions(("x-hypha-scopes" = json!(["carts:write"]))),
)]
pub async fn set_language<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller<Storefront>,
    PathId(id): PathId<CartId>,
    body: JsonBody<SetLanguageRequest>,
) -> Result<Localized<CartResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let store = super::stores::store_as_system(&state, &ports).await?;
    let language = match req.language.as_deref() {
        Some(asked) => {
            let cart = hypha_cart::find_cart(&ctx, &ports, id).await?;
            let market = ports.markets().get(cart.market()).await?;
            language::chosen(Some(asked), &store, market.as_ref())
        }
        None => None,
    };
    let priced = set_cart_language(&ctx, &ports, id, language).await?;
    let checks = state.cart_checks(&ports);
    let (served, response) = cart_answer(&state, &ports, &priced, &checks, &store).await?;
    Ok(served.respond(response))
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
    responses((status = 200, description = "OK", body = CartResponse, headers(("Content-Language" = String, description = "The language the response is in.")))),
    extensions(("x-hypha-scopes" = json!(["carts:write"]))),
)]
pub async fn set_marketing<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller<Storefront>,
    PathId(id): PathId<CartId>,
    body: JsonBody<SetMarketingRequest>,
) -> Result<Localized<CartResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let priced = hypha_cart::set_cart_marketing(&ctx, &ports, id, req.opt_in).await?;
    storefront_cart(&state, &ports, &priced, &state.cart_checks(&ports)).await
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
    let store = super::stores::store_as_system(&state, &ports).await?;
    // The admin's: as stored, in the store's language.
    let cart = cart_response(
        &priced,
        &ports,
        &state.cart_checks(&ports),
        &crate::language::content_languages(&store),
        state.media.as_deref(),
        None,
    )
    .await?;
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
    responses((status = 200, description = "OK", body = CartResponse, headers(("Content-Language" = String, description = "The language the response is in.")))),
    extensions(("x-hypha-scopes" = json!(["carts:write"]))),
)]
pub async fn set_quantity<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller<Storefront>,
    PathId((id, variant)): PathId<(CartId, VariantId)>,
    body: JsonBody<SetQuantityRequest>,
) -> Result<Localized<CartResponse>, ApiError> {
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
    storefront_cart(&state, &ports, &priced, &state.cart_checks(&ports)).await
}

/// Removes a line.
#[utoipa::path(
    delete,
    path = "/storefront/carts/{id}/lines/{variant}",
    params(("id" = CartId, Path), ("variant" = VariantId, Path)),
    responses((status = 200, description = "OK", body = CartResponse, headers(("Content-Language" = String, description = "The language the response is in.")))),
    extensions(("x-hypha-scopes" = json!(["carts:write"]))),
)]
pub async fn remove_line<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller<Storefront>,
    PathId((id, variant)): PathId<(CartId, VariantId)>,
) -> Result<Localized<CartResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let priced = remove_cart_line(&ctx, &ports, id, variant)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("line_not_found"))?;
    storefront_cart(&state, &ports, &priced, &state.cart_checks(&ports)).await
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
    responses((status = 200, description = "OK", body = CartResponse, headers(("Content-Language" = String, description = "The language the response is in.")))),
    extensions(("x-hypha-scopes" = json!(["carts:write"]))),
)]
pub async fn set_address<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller<Storefront>,
    PathId(id): PathId<CartId>,
    body: JsonBody<SetShippingAddressRequest>,
) -> Result<Localized<CartResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let mut priced = set_shipping_address(
        &ctx,
        &ports,
        &hypha_addresses::Formats,
        &state.verifier(&ports, Some(id.to_string())),
        id,
        req.address.map(Into::into),
    )
    .await?;
    let checks = state.cart_checks(&ports);
    let store = super::stores::store_as_system(&state, &ports).await?;
    hypha_checkout::validate_cart(
        &ctx,
        &ports,
        &checks,
        &crate::language::content_languages(&store),
        &state.clock,
        &mut priced,
        hypha_core::ValidationStep::Address,
    )
    .await?;
    let (served, response) = cart_answer(&state, &ports, &priced, &checks, &store).await?;
    Ok(served.respond(response))
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
    responses((status = 200, description = "OK", body = ShippingOptionsResponse, headers(("Content-Language" = String, description = "The language the response is in."))), (status = 429, description = "Over the rate limit for this cart (`rate_limited`); see `Retry-After`", body = ApiErrorBody)),
    extensions(("x-hypha-scopes" = json!(["carts:read"]))),
)]
pub async fn shipping_options<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller<Storefront>,
    PathId(id): PathId<CartId>,
) -> Result<Localized<ShippingOptionsResponse>, ApiError> {
    // After the key: a stranger can't use up a cart's quota.
    let (ports, ctx) = caller.authenticate(&state).await?;
    rate_limit::enforce(&state.limiter, Rule::ShippingQuotesCart, &id.to_string()).await?;
    let rates = state.cart_rates(&ports, id);
    let store = super::stores::store_as_system(&state, &ports).await?;
    let languages = crate::language::content_languages(&store);
    let offers = shipping_offers(&ctx, &ports, &rates, &languages, id).await?;
    // Priced for the offers: its market's languages are read already.
    let served = match offers.cart.order {
        None => ServedLanguage::of_cart(&offers.cart, &store),
        Some(order) => {
            served_checked_out(&ports, offers.cart.language.as_ref(), order, &store).await?
        }
    };
    let mut response = ShippingOptionsResponse {
        groups: offers.groups.iter().map(Into::into).collect(),
    };
    if !served.is_source() {
        let mut options: Vec<ResourceRef> = Vec::new();
        for group in &response.groups {
            let named = group.offers.iter().map(|o| o.option);
            for option in named.chain(group.unavailable.iter().map(|u| u.option)) {
                let r = ResourceRef::new(ResourceKind::ShippingOption, option.as_uuid());
                if !options.contains(&r) {
                    options.push(r);
                }
            }
        }
        let overlay = served.overlay(&ports, &options).await?;
        let name = |option: ShippingOptionId, name: &mut String| {
            let r = ResourceRef::new(ResourceKind::ShippingOption, option.as_uuid());
            if let Some(text) = overlay.text(r, keys::shipping_option::NAME) {
                *name = text.to_string();
            }
        };
        for group in &mut response.groups {
            for offer in &mut group.offers {
                name(offer.option, &mut offer.name);
            }
            for left_out in &mut group.unavailable {
                name(left_out.option, &mut left_out.name);
            }
        }
    }
    Ok(served.respond(response))
}

/// `e` as the shopper is told it: a refusal listing the cart's errors (`checkout_blocked`)
/// names checkout rules' messages in the language the cart reads in, as the cart shows
/// them. Anything else is answered as it is.
pub(crate) async fn shopper_error<S: Storage>(
    state: &crate::state::AppState<S>,
    ports: &S::Ports<'_>,
    ctx: &hypha_core::Ctx,
    id: CartId,
    e: hypha_checkout::CommandError,
) -> ApiError {
    let hypha_checkout::CommandError::CheckoutBlocked { cart, messages } = e else {
        return e.into();
    };
    let rules: Vec<ResourceRef> = messages
        .iter()
        .filter_map(|m| match m.source() {
            MessageSource::Rule { rule } => {
                Some(ResourceRef::new(ResourceKind::CheckoutRule, rule.as_uuid()))
            }
            _ => None,
        })
        .collect();
    let translated = async {
        if rules.is_empty() {
            return Ok(messages.clone());
        }
        let served = served_for(state, ports, ctx, id).await?;
        let overlay = served.overlay(ports, &rules).await?;
        Ok::<_, ApiError>(
            messages
                .iter()
                .map(|m| {
                    let MessageSource::Rule { rule } = m.source() else {
                        return m.clone();
                    };
                    let of_rule = ResourceRef::new(ResourceKind::CheckoutRule, rule.as_uuid());
                    overlay
                        .text(of_rule, keys::checkout_rule::MESSAGE)
                        .and_then(|text| {
                            Message::new(
                                m.severity(),
                                m.code().clone(),
                                m.path(),
                                text,
                                m.source().clone(),
                            )
                            .ok()
                        })
                        .unwrap_or_else(|| m.clone())
                })
                .collect(),
        )
    }
    .await;
    // Failing to translate shouldn't hide the refusal: it's answered in the source.
    let messages = translated.unwrap_or(messages);
    hypha_checkout::CommandError::CheckoutBlocked { cart, messages }.into()
}

/// The language the cart `id` reads in now, for a read about it that isn't the cart
/// itself (a refusal's messages): see `served_in_store`.
async fn served_for<S: Storage>(
    state: &crate::state::AppState<S>,
    ports: &S::Ports<'_>,
    ctx: &hypha_core::Ctx,
    id: CartId,
) -> Result<ServedLanguage, ApiError> {
    let store = super::stores::store_as_system(state, ports).await?;
    let cart = hypha_cart::find_cart(ctx, ports, id).await?;
    served_in_store(ports, &cart, &store).await
}

/// The language `cart` reads in now, for a read about it that isn't the cart itself (its
/// payment methods): the cart's rule (`ServedLanguage::of_cart_choice`) over its market's
/// languages, or its order's once checked out, without pricing it. A store serving only
/// its default reads nothing more.
async fn served_in_store<P: HasMarkets + HasOrders>(
    ports: &P,
    cart: &hypha_cart::Cart,
    store: &hypha_stores::Store,
) -> Result<ServedLanguage, ApiError> {
    match cart.status() {
        CartStatus::Completed { order } => {
            served_checked_out(ports, cart.language(), order, store).await
        }
        CartStatus::Open if one_language(store) => Ok(ServedLanguage::of_order(None, store)),
        CartStatus::Open => {
            let market_languages: Vec<Language> = ports
                .markets()
                .get(cart.market())
                .await?
                .map(|m| m.languages().to_vec())
                .unwrap_or_default();
            Ok(ServedLanguage::of_cart_choice(
                cart.language(),
                &market_languages,
                store,
            ))
        }
    }
}

/// `served_in_store` for a checked-out cart: its choice held to its order's language
/// alone (what `PricedCart::market_languages` holds then), that language while the store
/// still publishes it.
async fn served_checked_out<P: HasOrders>(
    ports: &P,
    language: Option<&Language>,
    order: OrderId,
    store: &hypha_stores::Store,
) -> Result<ServedLanguage, ApiError> {
    if one_language(store) {
        return Ok(ServedLanguage::of_order(None, store));
    }
    let order_language: Vec<Language> = ports
        .orders()
        .get(order)
        .await?
        .and_then(|o| o.language().cloned())
        .into_iter()
        .collect();
    Ok(ServedLanguage::of_cart_choice(
        language,
        &order_language,
        store,
    ))
}

/// Whether `store` serves its default language alone: every cart reads in it then.
fn one_language(store: &hypha_stores::Store) -> bool {
    store.published_languages().next().is_none()
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
    responses((status = 200, description = "OK", body = CartResponse, headers(("Content-Language" = String, description = "The language the response is in."))), (status = 429, description = "Over the rate limit for this cart (`rate_limited`); see `Retry-After`", body = ApiErrorBody)),
    extensions(("x-hypha-scopes" = json!(["carts:write"]))),
)]
pub async fn select<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller<Storefront>,
    PathId((id, group)): PathId<(CartId, ShippingGroupId)>,
    body: JsonBody<SelectShippingRequest>,
) -> Result<Localized<CartResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    rate_limit::enforce(&state.limiter, Rule::ShippingQuotesCart, &id.to_string()).await?;
    let req = body.parse()?;
    let rates = state.cart_rates(&ports, id);
    let store = super::stores::store_as_system(&state, &ports).await?;
    let languages = crate::language::content_languages(&store);
    let mut priced = select_shipping(
        &ctx,
        &ports,
        &rates,
        &languages,
        &state.clock,
        id,
        group,
        req.option,
    )
    .await?;
    let checks = state.cart_checks(&ports);
    hypha_checkout::validate_cart(
        &ctx,
        &ports,
        &checks,
        &languages,
        &state.clock,
        &mut priced,
        hypha_core::ValidationStep::Shipping,
    )
    .await?;
    let (served, response) = cart_answer(&state, &ports, &priced, &checks, &store).await?;
    Ok(served.respond(response))
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
    responses((status = 200, description = "OK", body = CartPaymentMethodsResponse, headers(("Content-Language" = String, description = "The language the response is in.")))),
    extensions(("x-hypha-scopes" = json!(["carts:read"]))),
)]
pub async fn payment_methods<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller<Storefront>,
    PathId(id): PathId<CartId>,
) -> Result<Localized<CartPaymentMethodsResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let providers = state.payments(&ports);
    let offered = hypha_checkout::cart_payment_methods(&ctx, &ports, &providers, id).await?;
    let store = super::stores::store_as_system(&state, &ports).await?;
    let served = served_in_store(&ports, &offered.cart, &store).await?;
    let methods = offered.methods;
    let resources: Vec<ResourceRef> = methods
        .iter()
        .map(|m| ResourceRef::new(ResourceKind::PaymentMethod, m.id().as_uuid()))
        .collect();
    let overlay = served.overlay(&ports, &resources).await?;
    Ok(served.respond(CartPaymentMethodsResponse {
        items: methods
            .iter()
            .map(|m| CartPaymentMethodResponse::translated(m, &overlay))
            .collect(),
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
    responses((status = 200, description = "OK", body = CartResponse, headers(("Content-Language" = String, description = "The language the response is in.")))),
    extensions(("x-hypha-scopes" = json!(["carts:write"]))),
)]
pub async fn set_payment_method<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller<Storefront>,
    PathId(id): PathId<CartId>,
    body: JsonBody<SetPaymentMethodRequest>,
) -> Result<Localized<CartResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let providers = state.payments(&ports);
    let priced =
        hypha_checkout::set_payment_method(&ctx, &ports, &providers, id, req.payment_method)
            .await
            // The method arrives in the body: invalid input, not the cart's state.
            .map_err(|e| ApiError::from(e).unprocessable_if("payment_method_unavailable"))?;
    storefront_cart(&state, &ports, &priced, &state.cart_checks(&ports)).await
}
