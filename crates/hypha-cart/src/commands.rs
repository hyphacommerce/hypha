use hypha_core::{
    AuthError, CartId, Ctx, IdGenerator, MarketId, ProductId, RepoError, Scope, VariantId,
};
use hypha_markets::{
    Catalog, CatalogContext, CatalogRepo, Market, MarketRepo, PriceList, PriceListRepo,
};
use hypha_products::{Product, ProductRepo};
use thiserror::Error;

use crate::{
    Cart, CartError, CartRepo, PriceCartError, PricedCart, Quantity, price_cart, price_line,
};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CommandError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error(transparent)]
    Cart(#[from] CartError),
    #[error(transparent)]
    Price(#[from] PriceCartError),
    #[error("cart {0} not found")]
    CartNotFound(CartId),
    #[error("market {0} not found")]
    MarketNotFound(MarketId),
}

pub async fn create_cart<M: MarketRepo, C: CartRepo, I: IdGenerator>(
    ctx: &Ctx,
    markets: &M,
    carts: &C,
    ids: &I,
    market: MarketId,
) -> Result<Cart, CommandError> {
    ctx.require(Scope::CartsWrite)?;

    if markets.get(market).await?.is_none() {
        return Err(CommandError::MarketNotFound(market));
    }

    let cart = Cart::new(CartId::from_uuid(ids.new_uuid()), market);
    carts.save(&cart).await?;
    Ok(cart)
}

pub struct AddLine {
    pub cart: CartId,
    pub product: ProductId,
    pub variant: VariantId,
    pub quantity: u32,
}

/// Adds a variant to the cart (or increases its quantity) and returns the repriced cart.
///
/// The variant must be sellable in the cart's market right now: the product is active,
/// owns the variant, and is in the market's catalog with a price in its currency.
pub async fn add_line<C, P, M, K, L>(
    ctx: &Ctx,
    carts: &C,
    products: &P,
    markets: &M,
    catalogs: &K,
    price_lists: &L,
    input: AddLine,
) -> Result<PricedCart, CommandError>
where
    C: CartRepo,
    P: ProductRepo,
    M: MarketRepo,
    K: CatalogRepo,
    L: PriceListRepo,
{
    ctx.require(Scope::CartsWrite)?;

    let quantity = Quantity::new(input.quantity)?;
    let mut cart = carts
        .get(input.cart)
        .await?
        .ok_or(CommandError::CartNotFound(input.cart))?;
    let product = products
        .get(input.product)
        .await?
        .ok_or(PriceCartError::ProductNotFound(input.product))?;

    let (market, catalogs, price_lists) =
        load_pricing(markets, catalogs, price_lists, cart.market()).await?;

    price_line(&product, input.variant, &market, &catalogs, &price_lists)?;
    cart.add_line(input.product, input.variant, quantity)?;

    // Price the whole cart before saving, so a failure stores nothing.
    let cart_products = load_products(products, &cart).await?;
    let priced = price_cart(&cart, &cart_products, &market, &catalogs, &price_lists)?;

    carts.save(&cart).await?;
    Ok(priced)
}

/// The cart at current prices.
pub async fn get_cart<C, P, M, K, L>(
    ctx: &Ctx,
    carts: &C,
    products: &P,
    markets: &M,
    catalogs: &K,
    price_lists: &L,
    id: CartId,
) -> Result<PricedCart, CommandError>
where
    C: CartRepo,
    P: ProductRepo,
    M: MarketRepo,
    K: CatalogRepo,
    L: PriceListRepo,
{
    ctx.require(Scope::CartsRead)?;

    let cart = carts.get(id).await?.ok_or(CommandError::CartNotFound(id))?;
    let (market, catalogs, price_lists) =
        load_pricing(markets, catalogs, price_lists, cart.market()).await?;
    let cart_products = load_products(products, &cart).await?;

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
async fn load_pricing<M: MarketRepo, K: CatalogRepo, L: PriceListRepo>(
    markets: &M,
    catalogs: &K,
    price_lists: &L,
    market_id: MarketId,
) -> Result<(Market, Vec<Catalog>, Vec<PriceList>), CommandError> {
    let market = markets
        .get(market_id)
        .await?
        .ok_or(CommandError::MarketNotFound(market_id))?;
    let catalog = catalogs
        .get_by_context(CatalogContext::Market(market_id))
        .await?;
    let price_list = match catalog.as_ref().and_then(|c| c.price_list()) {
        Some(id) => price_lists.get(id).await?,
        None => None,
    };
    Ok((
        market,
        catalog.into_iter().collect(),
        price_list.into_iter().collect(),
    ))
}

/// Loads each product in the cart once. Missing products are left out;
/// `price_cart` reports them as `ProductNotFound`.
async fn load_products<P: ProductRepo>(
    products: &P,
    cart: &Cart,
) -> Result<Vec<Product>, CommandError> {
    let mut loaded: Vec<Product> = Vec::new();
    for line in cart.lines() {
        if loaded.iter().any(|p| p.id() == line.product()) {
            continue;
        }
        if let Some(product) = products.get(line.product()).await? {
            loaded.push(product);
        }
    }
    Ok(loaded)
}
