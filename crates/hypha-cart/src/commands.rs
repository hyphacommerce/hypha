use hypha_core::{
    AuthError, CartId, Ctx, ErrorCode, IdGenerator, MarketId, MoneyError, RepoError, Scope,
    VariantId,
};
use hypha_markets::{
    Catalog, CatalogContext, CatalogRepo, Market, MarketRepo, PriceList, PriceListRepo,
};
use hypha_products::{Product, ProductRepo};
use thiserror::Error;

use crate::{Cart, CartError, CartRepo, LineError, PricedCart, Quantity, price_cart, price_line};

#[derive(Debug, Error, PartialEq, Eq)]
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
    #[error("cart {0} not found")]
    CartNotFound(CartId),
    #[error("market {0} not found")]
    MarketNotFound(MarketId),
    #[error("variant {0} not found")]
    VariantNotFound(VariantId),
}

impl ErrorCode for CommandError {
    fn code(&self) -> &'static str {
        match self {
            CommandError::Auth(e) => e.code(),
            CommandError::Repo(e) => e.code(),
            CommandError::Cart(e) => e.code(),
            CommandError::Line(e) => e.code(),
            CommandError::Money(e) => e.code(),
            CommandError::CartNotFound(_) => "cart_not_found",
            CommandError::MarketNotFound(_) => "market_not_found",
            CommandError::VariantNotFound(_) => "variant_not_found",
        }
    }
}

/// Every port the cart commands use, implemented once by whoever owns the repos
/// (a host's app state, a test world).
///
/// Associated types instead of `&dyn` keep dispatch static, so hosts can still
/// prove `Send` where they need it. Adding a port later touches this trait and
/// its impls, not every command signature.
pub trait CartPorts {
    type Carts: CartRepo;
    type Products: ProductRepo;
    type Markets: MarketRepo;
    type Catalogs: CatalogRepo;
    type PriceLists: PriceListRepo;

    fn carts(&self) -> &Self::Carts;
    fn products(&self) -> &Self::Products;
    fn markets(&self) -> &Self::Markets;
    fn catalogs(&self) -> &Self::Catalogs;
    fn price_lists(&self) -> &Self::PriceLists;
}

pub async fn create_cart<S: CartPorts, I: IdGenerator>(
    ctx: &Ctx,
    ports: &S,
    ids: &I,
    market: MarketId,
) -> Result<Cart, CommandError> {
    ctx.require(Scope::CartsWrite)?;

    if ports.markets().get(market).await?.is_none() {
        return Err(CommandError::MarketNotFound(market));
    }

    let cart = Cart::new(CartId::from_uuid(ids.new_uuid()), market);
    ports.carts().save(&cart).await?;
    Ok(cart)
}

pub struct AddLine {
    pub cart: CartId,
    pub variant: VariantId,
    pub quantity: u32,
}

/// Adds a variant to the cart (or increases its quantity) and returns the repriced cart.
///
/// Strict about the line being added: its product must be active and in the market's
/// catalog with a price in its currency. Other lines that went stale don't block it;
/// they come back marked unavailable.
pub async fn add_line<S: CartPorts>(
    ctx: &Ctx,
    ports: &S,
    input: AddLine,
) -> Result<PricedCart, CommandError> {
    ctx.require(Scope::CartsWrite)?;

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

    let (market, catalogs, price_lists) = load_pricing(ports, cart.market()).await?;

    price_line(&product, input.variant, &market, &catalogs, &price_lists)?;
    cart.add_line(product.id(), input.variant, quantity)?;

    // Price the whole cart before saving, so a failure (total overflow) stores nothing.
    let cart_products = load_products(ports, &cart).await?;
    let priced = price_cart(&cart, &cart_products, &market, &catalogs, &price_lists)?;

    ports.carts().save(&cart).await?;
    Ok(priced)
}

/// The cart at current prices. Stale lines are marked unavailable, not errors.
pub async fn get_cart<S: CartPorts>(
    ctx: &Ctx,
    ports: &S,
    id: CartId,
) -> Result<PricedCart, CommandError> {
    ctx.require(Scope::CartsRead)?;

    let cart = ports
        .carts()
        .get(id)
        .await?
        .ok_or(CommandError::CartNotFound(id))?;
    let (market, catalogs, price_lists) = load_pricing(ports, cart.market()).await?;
    let cart_products = load_products(ports, &cart).await?;

    Ok(price_cart(
        &cart,
        &cart_products,
        &market,
        &catalogs,
        &price_lists,
    )?)
}

/// The market plus its catalog and price list, if any. Missing ones are left out
/// rather than reported here: `resolve_price` turns them into precise errors.
async fn load_pricing<S: CartPorts>(
    ports: &S,
    market_id: MarketId,
) -> Result<(Market, Vec<Catalog>, Vec<PriceList>), CommandError> {
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
    Ok((
        market,
        catalog.into_iter().collect(),
        price_list.into_iter().collect(),
    ))
}

/// Loads each product in the cart once. Missing (deleted) products are left out;
/// `price_cart` marks their lines unavailable with `ProductNotFound`.
async fn load_products<S: CartPorts>(ports: &S, cart: &Cart) -> Result<Vec<Product>, CommandError> {
    let mut loaded: Vec<Product> = Vec::new();
    for line in cart.lines() {
        if loaded.iter().any(|p| p.id() == line.product()) {
            continue;
        }
        if let Some(product) = ports.products().get(line.product()).await? {
            loaded.push(product);
        }
    }
    Ok(loaded)
}
