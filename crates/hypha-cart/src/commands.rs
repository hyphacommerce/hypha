use hypha_core::{
    AddressError, AddressParts, AuthError, CartId, Clock, CountryCode, CountryError, Ctx,
    CustomerId, Email, EmailError, ErrorCode, HasBatch, HasClock, HasIdempotency, IdGenerator,
    IdempotencyError, IdempotencyRecord, IdempotencyRequest, Idempotent, MarketId, MoneyError,
    ProductId, Quantity, QuantityError, RepoError, Scope, ShippingGroupId, ShippingOptionId,
    StageIdempotency, Timestamp, VariantId, earlier_id,
};
use hypha_customers::{CustomerRepo, HasCustomers};
use hypha_inventory::{HasInventoryItems, HasLocations, StockContext};
use hypha_markets::{HasCatalogs, HasMarkets, HasPriceLists, Market, MarketRepo};
use hypha_pricing::PricingContext;
use hypha_products::{HasProducts, Product, ProductRepo};
use hypha_shipping::{
    HasShippingZones, NotOffered, OfferOutcome, RateItem, RateProvider, RateRequest,
    ShippingZoneRepo, offers,
};
use hypha_tax::{HasTaxClasses, HasTaxRegions, TaxClassRepo, TaxContext, TaxRegionRepo};
use thiserror::Error;

use crate::{
    Cart, CartError, CartRepo, HasCarts, LineError, LineStatus, PricedCart, Publication,
    SelectedShipping, ShippingContext, StageCarts, StoredQuote, check_stock,
    price_cart_with_shipping, price_line,
};
use hypha_channels::{ChannelRepo, HasChannels};

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum CommandError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error(transparent)]
    Cart(#[from] CartError),
    #[error(transparent)]
    Line(#[from] LineError),
    #[error(transparent)]
    Money(#[from] MoneyError),
    #[error(transparent)]
    Email(#[from] EmailError),
    #[error(transparent)]
    Country(#[from] CountryError),
    #[error(transparent)]
    Idempotency(#[from] IdempotencyError),
    #[error("customer {0} not found")]
    CustomerNotFound(CustomerId),
    #[error("cart {0} not found")]
    CartNotFound(CartId),
    #[error("market {0} not found")]
    MarketNotFound(MarketId),
    #[error("no market sells to {0}")]
    NoMarketForCountry(CountryCode),
    /// Carts are made by storefronts, which sell through a channel: a caller in no channel
    /// (staff, a secret key) can't make one.
    #[error("a cart is made in a sales channel: use a publishable key")]
    ChannelRequired,
    #[error("variant {0} not found")]
    VariantNotFound(VariantId),
    #[error(transparent)]
    Address(#[from] AddressError),
    #[error("shipping group {0} not found")]
    ShippingGroupNotFound(ShippingGroupId),
    /// Nothing in the cart needs shipping, so there's nothing to choose.
    #[error("nothing in the cart is shipped")]
    NothingToShip,
    #[error("no shipping to {0}")]
    NoShippingToCountry(CountryCode),
    #[error("shipping option {0} isn't one of the cart's zone")]
    ShippingOptionUnknown(ShippingOptionId),
    /// Known, but not offered to this cart right now (`reason`).
    #[error("shipping option {option} isn't offered to this cart: {}", reason.code())]
    ShippingOptionNotOffered {
        option: ShippingOptionId,
        reason: NotOffered,
    },
}

/// A quantity from the request is part of a cart line, so it reports like the cart's.
impl From<QuantityError> for CommandError {
    fn from(e: QuantityError) -> Self {
        CommandError::Cart(e.into())
    }
}

impl ErrorCode for CommandError {
    fn code(&self) -> &'static str {
        match self {
            CommandError::Auth(e) => e.code(),
            CommandError::Repo(e) => e.code(),
            CommandError::Cart(e) => e.code(),
            CommandError::Line(e) => e.code(),
            CommandError::Money(e) => e.code(),
            CommandError::Email(e) => e.code(),
            CommandError::Country(e) => e.code(),
            CommandError::Idempotency(e) => e.code(),
            CommandError::ChannelRequired => "channel_required",
            CommandError::CustomerNotFound(_) => "customer_not_found",
            CommandError::CartNotFound(_) => "cart_not_found",
            CommandError::MarketNotFound(_) => "market_not_found",
            CommandError::NoMarketForCountry(_) => "no_market_for_country",
            CommandError::VariantNotFound(_) => "variant_not_found",
            CommandError::Address(e) => e.code(),
            CommandError::ShippingGroupNotFound(_) => "shipping_group_not_found",
            CommandError::NothingToShip => "nothing_to_ship",
            CommandError::NoShippingToCountry(_) => "no_shipping_to_country",
            CommandError::ShippingOptionUnknown(_) => "shipping_option_unknown",
            CommandError::ShippingOptionNotOffered { .. } => "shipping_option_not_offered",
        }
    }
}

/// Every port the cart commands use, as one bound for hosts and test worlds: anything
/// with all the accessors is `CartPorts` automatically (the blanket impl below).
///
/// Each command binds only the accessors it uses (`create_cart` needs markets and
/// carts), so a new port, or a new composing crate like orders, doesn't touch every
/// signature or implementor. Associated types instead of `&dyn` keep dispatch static,
/// so hosts can still prove `Send` where they need it.
pub trait CartPorts:
    HasCarts
    + HasProducts
    + HasMarkets
    + HasCatalogs
    + HasPriceLists
    + HasTaxClasses
    + HasTaxRegions
    + HasInventoryItems
    + HasLocations
    + HasCustomers
    + IdempotentCartPorts
{
}

impl<T> CartPorts for T where
    T: HasCarts
        + HasProducts
        + HasMarkets
        + HasCatalogs
        + HasPriceLists
        + HasTaxClasses
        + HasTaxRegions
        + HasInventoryItems
        + HasLocations
        + HasCustomers
        + IdempotentCartPorts
        + ?Sized
{
}

/// What a cart command that takes an idempotency key needs: carts and pricing, the
/// recorded keys, and a batch that writes the cart and its key together.
pub trait IdempotentCartPorts:
    HasCarts + PricingPorts + HasIdempotency + HasBatch<Batch: StageCarts + StageIdempotency>
{
}

impl<T> IdempotentCartPorts for T where
    T: HasCarts
        + PricingPorts
        + HasIdempotency
        + HasBatch<Batch: StageCarts + StageIdempotency>
        + ?Sized
{
}

/// What pricing a cart reads: everything but the carts and customers themselves.
pub trait PricingPorts:
    HasClock
    + HasChannels
    + HasProducts
    + HasMarkets
    + HasCatalogs
    + HasPriceLists
    + HasTaxClasses
    + HasTaxRegions
    + HasInventoryItems
    + HasLocations
    + HasShippingZones
{
}

impl<T> PricingPorts for T where
    T: HasClock
        + HasChannels
        + HasProducts
        + HasMarkets
        + HasCatalogs
        + HasPriceLists
        + HasTaxClasses
        + HasTaxRegions
        + HasInventoryItems
        + HasLocations
        + HasShippingZones
        + ?Sized
{
}

/// The operation names idempotency records carry (`IdempotencyRecord::operation`).
pub const CREATE_CART: &str = "create_cart";
pub const ADD_LINE: &str = "add_line";

pub struct CreateCart {
    /// Where the cart ships to: an ISO 3166-1 alpha-2 code, checked here.
    pub country: String,
    /// A retry with the same key returns the cart the first request created.
    pub idempotency: Option<IdempotencyRequest>,
    /// The signed-in customer the cart is for, with their email: the host passes the
    /// session's customer; a guest's cart has none.
    pub customer: Option<(CustomerId, Email)>,
}

/// Creates an empty cart shipping to `country`, in the market that sells there
/// (like Shopify, the buyer's country picks the market), and returns it priced like
/// `get_cart`.
pub async fn create_cart<S: IdempotentCartPorts, I: IdGenerator, C: Clock>(
    ctx: &Ctx,
    ports: &S,
    ids: &I,
    clock: &C,
    input: CreateCart,
) -> Result<Idempotent<PricedCart>, CommandError> {
    ctx.require(Scope::CartsWrite)?;
    let key = input.idempotency.as_ref();
    if let Some(cart) = replay_of(ctx, ports, CREATE_CART, key).await? {
        return replay(ctx, ports, cart).await;
    }

    let channel = ctx.channel().ok_or(CommandError::ChannelRequired)?;
    let country = CountryCode::new(&input.country)?;
    let market = ports
        .markets()
        .get_by_country(country)
        .await?
        .ok_or(CommandError::NoMarketForCountry(country))?;

    let mut cart = Cart::new(
        CartId::from_uuid(ids.new_uuid()),
        market.id(),
        country,
        channel,
    );
    if input.customer.is_some() {
        cart.set_customer(input.customer)?;
    }
    // A new cart is empty: priced from the market just read, without reading it again.
    let priced = empty_reading(&cart, market).priced;
    match write_cart(
        ctx,
        ports,
        clock,
        CREATE_CART,
        key,
        &mut cart,
        Write::Insert,
    )
    .await?
    {
        Some(winner) => replay(ctx, ports, winner).await,
        None => Ok(Idempotent::fresh(priced)),
    }
}

pub struct AddLine {
    pub cart: CartId,
    pub variant: VariantId,
    pub quantity: u32,
    /// A retry with the same key returns the cart instead of adding the line again.
    pub idempotency: Option<IdempotencyRequest>,
}

/// Adds a variant to the cart (or increases its quantity) and returns the repriced cart.
///
/// Strict about the line being added: its product must be active and in the market's
/// catalog with a price in its currency, and the market's locations must have stock for the
/// line's new quantity. Other lines that went stale don't block it; they come back marked
/// unavailable.
pub async fn add_line<S: IdempotentCartPorts, C: Clock>(
    ctx: &Ctx,
    ports: &S,
    clock: &C,
    input: AddLine,
) -> Result<Idempotent<PricedCart>, CommandError> {
    ctx.require(Scope::CartsWrite)?;
    let key = input.idempotency.as_ref();
    if let Some(cart) = replay_of(ctx, ports, ADD_LINE, key).await? {
        return replay(ctx, ports, cart).await;
    }

    let quantity = Quantity::new(input.quantity)?;
    let mut cart = load_open_cart(ctx, ports, input.cart).await?;
    let product = ports
        .products()
        .get_by_variant(input.variant)
        .await?
        .ok_or(CommandError::VariantNotFound(input.variant))?;

    // Only changes the copy in memory; nothing is saved unless every check below passes.
    cart.add_line(product.id(), input.variant, quantity)?;
    let priced = reprice_grown_line(ports, &cart, product, input.variant).await?;

    // Fails with StaleVersion (409) if another request changed the cart meanwhile,
    // instead of silently dropping that request's line.
    match write_cart(ctx, ports, clock, ADD_LINE, key, &mut cart, Write::Update).await? {
        Some(winner) => replay(ctx, ports, winner).await,
        None => Ok(Idempotent::fresh(priced)),
    }
}

/// The cart an earlier request produced, if this caller already sent `request` for
/// `operation`; `KeyReused` if the key was used for something else.
async fn replay_of<S: HasIdempotency>(
    ctx: &Ctx,
    ports: &S,
    operation: &str,
    request: Option<&IdempotencyRequest>,
) -> Result<Option<CartId>, CommandError> {
    earlier_id(
        ports.idempotency(),
        ctx.actor(),
        operation,
        request,
        CartId::from_uuid,
    )
    .await
}

/// What a replay returns: the cart as it is now (not as the first response showed it).
async fn replay<S: HasCarts + PricingPorts>(
    ctx: &Ctx,
    ports: &S,
    cart: CartId,
) -> Result<Idempotent<PricedCart>, CommandError> {
    let cart = load_cart(ctx, ports, cart).await?;
    Ok(Idempotent::replayed(reprice(ports, &cart).await?))
}

enum Write {
    Insert,
    Update,
}

/// Saves `cart`. With an idempotency key, the cart and the key's record commit together,
/// so a retry finds both or neither. If another request with the same key committed
/// between `replay_of` and this write, returns its cart to replay instead.
async fn write_cart<S: IdempotentCartPorts, C: Clock>(
    ctx: &Ctx,
    ports: &S,
    clock: &C,
    operation: &str,
    request: Option<&IdempotencyRequest>,
    cart: &mut Cart,
    write: Write,
) -> Result<Option<CartId>, CommandError> {
    let Some(request) = request else {
        match write {
            Write::Insert => ports.carts().insert(cart).await?,
            Write::Update => ports.carts().update(cart).await?,
        }
        return Ok(None);
    };

    let record = IdempotencyRecord::new(
        ctx.actor(),
        request,
        operation,
        cart.id().as_uuid(),
        clock.now(),
    )?;
    let mut batch = ports.batch();
    match write {
        Write::Insert => batch.insert_cart(cart),
        Write::Update => batch.update_cart(cart),
    }
    batch.record_idempotency(record);
    match ports.commit(batch).await {
        Ok(()) => Ok(None),
        // A request with the same key may have committed first. Its write is then what
        // failed this one (the key is taken, or the cart's version moved on), and its
        // cart is this request's answer too.
        Err(e) => match replay_of(ctx, ports, operation, Some(request)).await? {
            Some(winner) => Ok(Some(winner)),
            None => Err(e.into()),
        },
    }
}

/// Sets who to contact about the order this cart becomes; `None` clears it.
pub async fn set_cart_email<S: HasCarts + PricingPorts>(
    ctx: &Ctx,
    ports: &S,
    id: CartId,
    email: Option<String>,
) -> Result<PricedCart, CommandError> {
    ctx.require(Scope::CartsWrite)?;

    let email = email.as_deref().map(Email::new).transpose()?;
    let mut cart = load_open_cart(ctx, ports, id).await?;
    cart.set_email(email)?;
    let priced = reprice(ports, &cart).await?;
    ports.carts().update(&mut cart).await?;
    Ok(priced)
}

/// Ticks or unticks the cart's marketing box: checkout records the consent when it's
/// ticked.
pub async fn set_cart_marketing<S: HasCarts + PricingPorts>(
    ctx: &Ctx,
    ports: &S,
    id: CartId,
    opt_in: bool,
) -> Result<PricedCart, CommandError> {
    ctx.require(Scope::CartsWrite)?;

    let mut cart = load_open_cart(ctx, ports, id).await?;
    cart.set_marketing_opt_in(opt_in)?;
    let priced = reprice(ports, &cart).await?;
    ports.carts().update(&mut cart).await?;
    Ok(priced)
}

/// Links the cart to an existing customer (taking their email), or unlinks it with `None`.
/// Linking needs `customers:read` as well as `carts:write`.
pub async fn set_cart_customer<S: HasCarts + HasCustomers + PricingPorts>(
    ctx: &Ctx,
    ports: &S,
    id: CartId,
    customer: Option<CustomerId>,
) -> Result<PricedCart, CommandError> {
    ctx.require(Scope::CartsWrite)?;

    // Linking copies the customer's email onto the cart and reveals whether the ID
    // exists, so it needs read access to customers, checked before any lookup.
    if customer.is_some() {
        ctx.require(Scope::CustomersRead)?;
    }

    let mut cart = load_open_cart(ctx, ports, id).await?;
    let buyer = match customer {
        Some(customer) => {
            let found = ports
                .customers()
                .get(customer)
                .await?
                .ok_or(CommandError::CustomerNotFound(customer))?;
            Some((found.id(), found.email().clone()))
        }
        None => None,
    };
    cart.set_customer(buyer)?;
    let priced = reprice(ports, &cart).await?;
    ports.carts().update(&mut cart).await?;
    Ok(priced)
}

pub struct SetLineQuantity {
    pub cart: CartId,
    pub variant: VariantId,
    /// At least 1; `remove_cart_line` takes a line out.
    pub quantity: u32,
}

/// Changes how many of a variant the cart holds and returns the repriced cart.
///
/// Raising the quantity is as strict as `add_line` (the line must be sellable and in
/// stock for its new quantity); lowering it is always allowed, so a buyer can shrink a
/// line that went out of stock.
pub async fn set_line_quantity<S: HasCarts + PricingPorts>(
    ctx: &Ctx,
    ports: &S,
    input: SetLineQuantity,
) -> Result<PricedCart, CommandError> {
    ctx.require(Scope::CartsWrite)?;

    let quantity = Quantity::new(input.quantity)?;
    let mut cart = load_open_cart(ctx, ports, input.cart).await?;
    let raised = quantity > cart.line(input.variant)?.quantity();
    cart.set_quantity(input.variant, quantity)?;

    let priced = if raised {
        let line = cart.line(input.variant)?;
        let product = ports
            .products()
            .get(line.product())
            .await?
            .ok_or(LineError::ProductNotFound(line.product()))?;
        reprice_grown_line(ports, &cart, product, input.variant).await?
    } else {
        reprice(ports, &cart).await?
    };

    ports.carts().update(&mut cart).await?;
    Ok(priced)
}

/// Takes a variant out of the cart and returns the repriced cart.
pub async fn remove_cart_line<S: HasCarts + PricingPorts>(
    ctx: &Ctx,
    ports: &S,
    cart: CartId,
    variant: VariantId,
) -> Result<PricedCart, CommandError> {
    ctx.require(Scope::CartsWrite)?;

    let mut cart = load_open_cart(ctx, ports, cart).await?;
    cart.remove_line(variant)?;
    let priced = reprice(ports, &cart).await?;
    ports.carts().update(&mut cart).await?;
    Ok(priced)
}

/// Reprices `cart` after `variant`'s line was added or grew, strictly: unlike `reprice`,
/// that line must be sellable, in stock for its whole quantity and fit the totals, or
/// this fails. `product` is the line's product, which the caller loaded already, so
/// each query runs once.
async fn reprice_grown_line<S: PricingPorts>(
    ports: &S,
    cart: &Cart,
    product: Product,
    variant: VariantId,
) -> Result<PricedCart, CommandError> {
    // Sold through the cart's channel: checked first, so a product the channel doesn't sell
    // says nothing about its price or stock.
    let published = load_publication(ports, cart).await?;
    if !published.allows(product.id()) {
        return Err(LineError::ProductNotPublished(product.id()).into());
    }
    let pricing = load_pricing(ports, cart).await?;
    price_line(&product, variant, cart.line(variant)?.quantity(), &pricing)?;

    // The whole line counts: adding 1 to a line of 2 needs 3 in stock. Nothing is
    // reserved; stock is only taken at checkout.
    let stock = load_stock(ports, cart).await?;
    check_stock(&stock, variant, cart.line(variant)?.quantity())?;

    let tax = load_tax(ports, cart, &pricing.market).await?;
    let others: Vec<ProductId> = product_ids(cart)
        .into_iter()
        .filter(|id| *id != product.id())
        .collect();
    let mut cart_products = ports.products().get_many(&others).await?;
    cart_products.push(product);
    let shipping = ShippingContext {
        zone: ports
            .shipping_zones()
            .get_by_country(cart.country())
            .await?,
    };
    let priced = price_cart_with_shipping(
        cart,
        &cart_products,
        &pricing,
        &tax,
        &stock,
        &shipping,
        &published,
    );

    refuse_unavailable(&priced, variant)?;
    Ok(priced)
}

/// The strict checks passed, so if `variant`'s line still comes back unavailable, its
/// total (or the cart's with it) doesn't fit: refuse it rather than store it.
fn refuse_unavailable(priced: &PricedCart, variant: VariantId) -> Result<(), CommandError> {
    match priced
        .lines
        .iter()
        .find(|l| l.variant == variant)
        .map(|l| &l.status)
    {
        Some(LineStatus::Unavailable(reason)) => Err(CommandError::Line(reason.clone())),
        _ => Ok(()),
    }
}

/// The cart, if the caller may use it: another channel's cart reads as missing.
async fn load_cart<S: HasCarts>(ctx: &Ctx, ports: &S, id: CartId) -> Result<Cart, CommandError> {
    ports
        .carts()
        .get(id)
        .await?
        .filter(|cart| cart.visible_in(ctx.channel()))
        .ok_or(CommandError::CartNotFound(id))
}

/// `load_cart` for a command that changes the cart: a checked-out cart is refused here,
/// before lookups that would report a less useful error.
async fn load_open_cart<S: HasCarts>(
    ctx: &Ctx,
    ports: &S,
    id: CartId,
) -> Result<Cart, CommandError> {
    let cart = load_cart(ctx, ports, id).await?;
    cart.ensure_open()?;
    Ok(cart)
}

/// The cart as stored, if the caller may use it (another channel's reads as missing),
/// without pricing it: for passing who and where a shopper is on to an app.
pub async fn find_cart<S: HasCarts>(
    ctx: &Ctx,
    ports: &S,
    id: CartId,
) -> Result<Cart, CommandError> {
    ctx.require(Scope::CartsRead)?;
    load_cart(ctx, ports, id).await
}

/// The cart at current prices, tax rates and stock. Stale lines are marked unavailable,
/// not errors.
pub async fn get_cart<S: HasCarts + PricingPorts>(
    ctx: &Ctx,
    ports: &S,
    id: CartId,
) -> Result<PricedCart, CommandError> {
    ctx.require(Scope::CartsRead)?;

    let cart = load_cart(ctx, ports, id).await?;
    reprice(ports, &cart).await
}

/// `cart` at current prices, tax rates and stock, in a fixed number of queries. Commands
/// that change a cart return this, so callers get the same shape as `get_cart`.
async fn reprice<S: PricingPorts>(ports: &S, cart: &Cart) -> Result<PricedCart, CommandError> {
    Ok(price_for_checkout(ports, cart).await?.0)
}

/// `cart` priced like `get_cart`, with the stock it was checked against. Not a command
/// (no scope check): checkout calls it, then reserves from that same stock, so pricing
/// and reservation see one read.
pub async fn price_for_checkout<S: PricingPorts>(
    ports: &S,
    cart: &Cart,
) -> Result<(PricedCart, StockContext), CommandError> {
    let reading = read_cart(ports, cart).await?;
    Ok((reading.priced, reading.stock))
}

/// A cart priced, with what it was priced from: what offers and checkout build rate
/// requests from.
pub struct CartReading {
    pub priced: PricedCart,
    pub stock: StockContext,
    pub products: Vec<Product>,
    pub shipping: ShippingContext,
}

/// Prices `cart` in a fixed number of queries (see `load_pricing`), its shipping too.
/// An empty cart reads only its market (`empty_reading`).
pub async fn read_cart<S: PricingPorts>(
    ports: &S,
    cart: &Cart,
) -> Result<CartReading, CommandError> {
    if cart.lines().is_empty() {
        let market = ports
            .markets()
            .get(cart.market())
            .await?
            .ok_or(CommandError::MarketNotFound(cart.market()))?;
        return Ok(empty_reading(cart, market));
    }
    let pricing = load_pricing(ports, cart).await?;
    let tax = load_tax(ports, cart, &pricing.market).await?;
    let stock = load_stock(ports, cart).await?;
    let products = load_products(ports, cart).await?;
    let shipping = ShippingContext {
        zone: ports
            .shipping_zones()
            .get_by_country(cart.country())
            .await?,
    };
    let published = load_publication(ports, cart).await?;
    let priced = price_cart_with_shipping(
        cart, &products, &pricing, &tax, &stock, &shipping, &published,
    );
    Ok(CartReading {
        priced,
        stock,
        products,
        shipping,
    })
}

/// `cart`, which has no lines, priced from its `market` alone. Without lines nothing
/// else can change the result: the market gives the currency and whether prices include
/// tax, and the catalog, price list, tax rates, stock, products, publications and
/// shipping zone only ever apply to lines (no line needs shipping, so there's no group
/// to offer options for). Saves a new or emptied cart the dozen or so reads of a full
/// `read_cart`.
fn empty_reading(cart: &Cart, market: Market) -> CartReading {
    debug_assert!(cart.lines().is_empty());
    // No line is priced, so the time doesn't matter.
    let pricing = PricingContext::empty(market, Timestamp::UNIX_EPOCH);
    let tax = TaxContext {
        region: None,
        prices_include_tax: pricing.market.prices_include_tax(),
        product_classes: Default::default(),
    };
    let stock = StockContext::default();
    let shipping = ShippingContext::default();
    let published = Publication::new(Default::default());
    let priced = price_cart_with_shipping(cart, &[], &pricing, &tax, &stock, &shipping, &published);
    CartReading {
        priced,
        stock,
        products: Vec::new(),
        shipping,
    }
}

/// Which of the cart's products its channel sells, in one query.
async fn load_publication<S: HasChannels>(
    ports: &S,
    cart: &Cart,
) -> Result<Publication, CommandError> {
    let products = product_ids(cart);
    if products.is_empty() {
        return Ok(Publication::new(Default::default()));
    }
    Ok(Publication::new(
        ports
            .channels()
            .published(cart.channel(), &products)
            .await?,
    ))
}

/// Everything needed to price the cart's lines (`hypha_pricing::load_pricing`).
/// With `load_tax`, `load_stock` and `load_products`, pricing a cart takes the same
/// number of round trips for 1 line or 100 (on Postgres about 20, counting the
/// BEGIN/COMMIT of each multi-query load); an empty cart reads only its market
/// (`empty_reading`).
async fn load_pricing<S: HasMarkets + HasCatalogs + HasPriceLists + HasClock>(
    ports: &S,
    cart: &Cart,
) -> Result<PricingContext, CommandError> {
    let variants: Vec<VariantId> = cart.lines().iter().map(|l| l.variant()).collect();
    hypha_pricing::load_pricing(ports, cart.market(), &product_ids(cart), &variants)
        .await?
        .ok_or(CommandError::MarketNotFound(cart.market()))
}

/// The tax rates of the cart's country and its products' tax classes, in 2 queries.
/// The market says whether its prices already include tax.
async fn load_tax<S: HasTaxClasses + HasTaxRegions>(
    ports: &S,
    cart: &Cart,
    market: &Market,
) -> Result<TaxContext, CommandError> {
    Ok(TaxContext {
        region: ports.tax_regions().get_by_country(cart.country()).await?,
        prices_include_tax: market.prices_include_tax(),
        product_classes: ports.tax_classes().classes_for(&product_ids(cart)).await?,
    })
}

/// The stock of the cart's variants in the locations serving its market.
async fn load_stock<S: HasInventoryItems + HasLocations>(
    ports: &S,
    cart: &Cart,
) -> Result<StockContext, CommandError> {
    let variants: Vec<VariantId> = cart.lines().iter().map(|l| l.variant()).collect();
    Ok(StockContext::load(ports, cart.market(), &variants).await?)
}

/// The cart's products, in one query. Missing (deleted) products are left out;
/// `price_cart` marks their lines unavailable with `ProductNotFound`.
async fn load_products<S: HasProducts>(
    ports: &S,
    cart: &Cart,
) -> Result<Vec<Product>, CommandError> {
    Ok(ports.products().get_many(&product_ids(cart)).await?)
}

/// Each product in the cart once.
fn product_ids(cart: &Cart) -> Vec<ProductId> {
    let mut ids: Vec<ProductId> = cart.lines().iter().map(|l| l.product()).collect();
    ids.sort_by_key(|p| p.as_uuid());
    ids.dedup();
    ids
}

// ---------- shipping ----------

/// Sets where the cart ships, or clears it with `None`. It must be in the cart's
/// country; a new address drops the shipping choice.
pub async fn set_shipping_address<S: HasCarts + PricingPorts>(
    ctx: &Ctx,
    ports: &S,
    id: CartId,
    address: Option<AddressParts>,
) -> Result<PricedCart, CommandError> {
    ctx.require(Scope::CartsWrite)?;
    let address = address.map(hypha_core::Address::new).transpose()?;
    let mut cart = load_open_cart(ctx, ports, id).await?;
    cart.set_shipping_address(address)?;
    let priced = reprice(ports, &cart).await?;
    ports.carts().update(&mut cart).await?;
    Ok(priced)
}

/// What one group of a cart can pick, priced now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupOffers {
    pub group: ShippingGroupId,
    pub offers: Vec<OfferOutcome>,
}

/// Every option the cart's zone offers its groups, priced: built-in rates computed,
/// providers asked (each once, all at once). A provider that fails leaves its options out
/// with the reason; it never fails the call.
pub async fn shipping_offers<S: HasCarts + PricingPorts, P: RateProvider>(
    ctx: &Ctx,
    ports: &S,
    rates: &P,
    id: CartId,
) -> Result<Vec<GroupOffers>, CommandError> {
    ctx.require(Scope::CartsRead)?;
    let cart = load_cart(ctx, ports, id).await?;
    let reading = read_cart(ports, &cart).await?;
    group_offers(&cart, &reading, rates).await
}

/// Picks `option` for `group`, if the cart is offered it now. A provider's price is stored
/// with the choice, as quoted.
pub async fn select_shipping<S: HasCarts + PricingPorts, P: RateProvider>(
    ctx: &Ctx,
    ports: &S,
    rates: &P,
    clock: &impl Clock,
    id: CartId,
    group: ShippingGroupId,
    option: ShippingOptionId,
) -> Result<PricedCart, CommandError> {
    ctx.require(Scope::CartsWrite)?;
    let mut cart = load_open_cart(ctx, ports, id).await?;
    if group != cart.shipping_group() {
        return Err(CommandError::ShippingGroupNotFound(group));
    }
    let reading = read_cart(ports, &cart).await?;
    let offered = group_offers(&cart, &reading, rates).await?;
    let outcome = offered
        .iter()
        .flat_map(|g| &g.offers)
        .find(|o| match o {
            OfferOutcome::Offered(offer) => offer.option == option,
            OfferOutcome::NotOffered { option: id, .. } => *id == option,
        })
        .ok_or(CommandError::ShippingOptionUnknown(option))?;
    let quote = match outcome {
        OfferOutcome::NotOffered { reason, .. } => {
            return Err(CommandError::ShippingOptionNotOffered {
                option,
                reason: reason.clone(),
            });
        }
        OfferOutcome::Offered(offer) => match &offer.quote {
            None => None,
            Some(source) => Some(StoredQuote {
                amount: hypha_core::Price::new(offer.amount)?,
                provider: source.provider.clone(),
                service: source.service.clone(),
                reference: source.reference.clone(),
                quoted_at: clock.now(),
            }),
        },
    };
    cart.select_shipping(Some(SelectedShipping { option, quote }))?;
    let priced = reprice(ports, &cart).await?;
    ports.carts().update(&mut cart).await?;
    Ok(priced)
}

/// The offers of each group of a priced cart. Needs an address and a zone for it.
pub async fn group_offers<P: RateProvider>(
    cart: &Cart,
    reading: &CartReading,
    rates: &P,
) -> Result<Vec<GroupOffers>, CommandError> {
    let groups = &reading.priced.shipping_groups;
    if groups.is_empty() {
        return Err(CommandError::NothingToShip);
    }
    let destination = cart
        .shipping_address()
        .cloned()
        .ok_or(CartError::ShippingAddressRequired)?;
    let zone = reading
        .shipping
        .zone
        .as_ref()
        .ok_or(CommandError::NoShippingToCountry(cart.country()))?;
    // The origin: the first location serving the market (by priority) with an address.
    let origin = reading
        .stock
        .locations
        .iter()
        .find_map(|l| l.address().cloned());

    let mut result = Vec::with_capacity(groups.len());
    for group in groups {
        let mut items = Vec::with_capacity(group.lines.len());
        let mut subtotal = hypha_core::Money::zero(reading.priced.currency);
        for line in reading
            .priced
            .lines
            .iter()
            .filter(|l| group.lines.contains(&l.variant))
        {
            let LineStatus::Priced {
                unit_price, total, ..
            } = &line.status
            else {
                continue;
            };
            let variant = reading
                .products
                .iter()
                .find(|p| p.id() == line.product)
                .and_then(|p| p.variant(line.variant));
            subtotal = subtotal.checked_add(*total)?;
            items.push(RateItem {
                variant: line.variant,
                sku: variant.and_then(|v| v.sku()).map(str::to_string),
                quantity: line.quantity,
                unit_price: *unit_price,
                weight: variant.and_then(|v| v.weight()),
            });
        }
        let request = RateRequest {
            currency: reading.priced.currency,
            origin: origin.clone(),
            destination: destination.clone(),
            items,
            subtotal,
        };
        result.push(GroupOffers {
            group: group.id,
            offers: offers(zone, rates, &request).await,
        });
    }
    Ok(result)
}
