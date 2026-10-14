use std::collections::{HashMap, HashSet};

use hypha_core::{
    AuthError, CartId, Clock, CountryCode, CountryError, Ctx, CustomerId, Email, EmailError,
    ErrorCode, HasBatch, HasIdempotency, IdGenerator, IdempotencyError, IdempotencyRecord,
    IdempotencyRequest, Idempotent, MarketId, MoneyError, ProductId, Quantity, QuantityError,
    RepoError, Scope, StageIdempotency, VariantId, earlier_result,
    idempotency::keys::IDEMPOTENCY_KEY,
};
use hypha_customers::{CustomerRepo, HasCustomers};
use hypha_inventory::{
    HasInventoryItems, HasLocations, InventoryItemRepo, LocationRepo, StockContext,
};
use hypha_markets::{
    CatalogContext, CatalogRepo, HasCatalogs, HasMarkets, HasPriceLists, Market, MarketRepo,
    PriceListRepo,
};
use hypha_pricing::PricingContext;
use hypha_products::{HasProducts, Product, ProductRepo};
use hypha_tax::{HasTaxClasses, HasTaxRegions, TaxClassRepo, TaxContext, TaxRegionRepo};
use thiserror::Error;

use crate::{
    Cart, CartError, CartRepo, HasCarts, LineError, LineStatus, PricedCart, StageCarts,
    check_stock, price_cart, price_line,
};

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
    #[error("variant {0} not found")]
    VariantNotFound(VariantId),
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
            CommandError::CustomerNotFound(_) => "customer_not_found",
            CommandError::CartNotFound(_) => "cart_not_found",
            CommandError::MarketNotFound(_) => "market_not_found",
            CommandError::NoMarketForCountry(_) => "no_market_for_country",
            CommandError::VariantNotFound(_) => "variant_not_found",
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
    HasProducts
    + HasMarkets
    + HasCatalogs
    + HasPriceLists
    + HasTaxClasses
    + HasTaxRegions
    + HasInventoryItems
    + HasLocations
{
}

impl<T> PricingPorts for T where
    T: HasProducts
        + HasMarkets
        + HasCatalogs
        + HasPriceLists
        + HasTaxClasses
        + HasTaxRegions
        + HasInventoryItems
        + HasLocations
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
        return replay(ports, cart).await;
    }

    let country = CountryCode::new(&input.country)?;
    let market = ports
        .markets()
        .get_by_country(country)
        .await?
        .ok_or(CommandError::NoMarketForCountry(country))?;

    let mut cart = Cart::new(CartId::from_uuid(ids.new_uuid()), market.id(), country);
    let priced = reprice(ports, &cart).await?;
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
        Some(winner) => replay(ports, winner).await,
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
/// catalog with a price in its currency, and the market's locations must have stock
/// for the line's new quantity (design note 0004, I6). Other lines that went stale
/// don't block it; they come back marked unavailable.
pub async fn add_line<S: IdempotentCartPorts, C: Clock>(
    ctx: &Ctx,
    ports: &S,
    clock: &C,
    input: AddLine,
) -> Result<Idempotent<PricedCart>, CommandError> {
    ctx.require(Scope::CartsWrite)?;
    let key = input.idempotency.as_ref();
    if let Some(cart) = replay_of(ctx, ports, ADD_LINE, key).await? {
        return replay(ports, cart).await;
    }

    let quantity = Quantity::new(input.quantity)?;
    let mut cart = ports
        .carts()
        .get(input.cart)
        .await?
        .ok_or(CommandError::CartNotFound(input.cart))?;
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
        Some(winner) => replay(ports, winner).await,
        None => Ok(Idempotent::fresh(priced)),
    }
}

/// The cart an earlier request produced, if this caller already sent `request` for
/// `operation`; `KeyReused` if the key was used for something else.
async fn replay_of<S: IdempotentCartPorts>(
    ctx: &Ctx,
    ports: &S,
    operation: &str,
    request: Option<&IdempotencyRequest>,
) -> Result<Option<CartId>, CommandError> {
    let Some(request) = request else {
        return Ok(None);
    };
    let earlier: Option<_> =
        earlier_result::<_, CommandError>(ports.idempotency(), ctx.actor(), operation, request)
            .await?;
    Ok(earlier.map(CartId::from_uuid))
}

/// What a replay returns: the cart as it is now (not as the first response showed it;
/// design note 0007, section 5).
async fn replay<S: HasCarts + PricingPorts>(
    ports: &S,
    cart: CartId,
) -> Result<Idempotent<PricedCart>, CommandError> {
    let cart = load_cart(ports, cart).await?;
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
        Err(RepoError::Duplicate(IDEMPOTENCY_KEY)) => {
            match replay_of(ctx, ports, operation, Some(request)).await? {
                Some(winner) => Ok(Some(winner)),
                None => Err(RepoError::Duplicate(IDEMPOTENCY_KEY).into()),
            }
        }
        written => Ok(written.map(|()| None)?),
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
    let mut cart = load_cart(ports, id).await?;
    cart.set_email(email)?;
    let priced = reprice(ports, &cart).await?;
    ports.carts().update(&mut cart).await?;
    Ok(priced)
}

/// Links the cart to an existing customer (taking their email), or unlinks it with
/// `None` (design note 0005, C5). Linking needs `customers:read` as well as `carts:write`.
pub async fn set_cart_customer<S: CartPorts>(
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

    let mut cart = load_cart(ports, id).await?;
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
    cart.set_customer(buyer);
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
    let mut cart = load_cart(ports, input.cart).await?;
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

    let mut cart = load_cart(ports, cart).await?;
    if !cart.remove_line(variant) {
        return Err(CartError::LineNotFound(variant).into());
    }
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
    let pricing = load_pricing(ports, cart).await?;
    price_line(&product, variant, &pricing)?;

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
    let priced = price_cart(cart, &cart_products, &pricing, &tax, &stock);

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

async fn load_cart<S: HasCarts>(ports: &S, id: CartId) -> Result<Cart, CommandError> {
    ports
        .carts()
        .get(id)
        .await?
        .ok_or(CommandError::CartNotFound(id))
}

/// The cart at current prices, tax rates and stock. Stale lines are marked unavailable,
/// not errors.
pub async fn get_cart<S: HasCarts + PricingPorts>(
    ctx: &Ctx,
    ports: &S,
    id: CartId,
) -> Result<PricedCart, CommandError> {
    ctx.require(Scope::CartsRead)?;

    let cart = load_cart(ports, id).await?;
    reprice(ports, &cart).await
}

/// `cart` at current prices, tax rates and stock, in a fixed number of queries. Commands
/// that change a cart return this, so callers get the same shape as `get_cart`.
async fn reprice<S: PricingPorts>(ports: &S, cart: &Cart) -> Result<PricedCart, CommandError> {
    let pricing = load_pricing(ports, cart).await?;
    let tax = load_tax(ports, cart, &pricing.market).await?;
    let stock = load_stock(ports, cart).await?;
    let cart_products = load_products(ports, cart).await?;
    Ok(price_cart(cart, &cart_products, &pricing, &tax, &stock))
}

/// Everything needed to price the cart's lines, with a fixed number of queries
/// however many lines there are: membership and fixed prices are fetched in batches.
/// With `load_tax`, `load_stock` and `load_products`, pricing a cart takes the same
/// number of round trips for 1 line or 100 (on Postgres about 20, counting the
/// BEGIN/COMMIT of each multi-query load); an empty cart skips the batch lookups.
///
/// A missing catalog or price list is left out rather than reported here:
/// `resolve_price` turns it into a precise per-line reason.
async fn load_pricing<S: HasMarkets + HasCatalogs + HasPriceLists>(
    ports: &S,
    cart: &Cart,
) -> Result<PricingContext, CommandError> {
    let market_id = cart.market();
    let market = ports
        .markets()
        .get(market_id)
        .await?
        .ok_or(CommandError::MarketNotFound(market_id))?;
    let catalog = ports
        .catalogs()
        .get_by_context(CatalogContext::Market(market_id))
        .await?;
    let price_list = match catalog.as_ref().and_then(|c| c.price_list()) {
        Some(id) => ports.price_lists().get(id).await?,
        None => None,
    };

    let products = product_ids(cart);
    let variants: Vec<VariantId> = cart.lines().iter().map(|l| l.variant()).collect();

    let included_products = match &catalog {
        Some(c) => {
            ports
                .catalogs()
                .included_products(c.id(), &products)
                .await?
        }
        None => HashSet::new(),
    };
    let fixed_prices = match &price_list {
        Some(l) => ports.price_lists().fixed_prices(l.id(), &variants).await?,
        None => HashMap::new(),
    };

    Ok(PricingContext {
        market,
        catalog,
        price_list,
        included_products,
        fixed_prices,
    })
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

/// The stock of the cart's variants in the locations serving its market, in 2 queries.
async fn load_stock<S: HasInventoryItems + HasLocations>(
    ports: &S,
    cart: &Cart,
) -> Result<StockContext, CommandError> {
    let variants: Vec<VariantId> = cart.lines().iter().map(|l| l.variant()).collect();
    let items = ports.inventory_items().for_variants(&variants).await?;
    Ok(StockContext {
        items: items.into_iter().map(|i| (i.variant(), i)).collect(),
        locations: ports.locations().serving(cart.market()).await?,
    })
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
