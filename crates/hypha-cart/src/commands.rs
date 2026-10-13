use std::collections::{HashMap, HashSet};

use hypha_core::{
    AuthError, CartId, CountryCode, CountryError, Ctx, CustomerId, Email, EmailError, ErrorCode,
    IdGenerator, MarketId, MoneyError, ProductId, RepoError, Scope, VariantId,
};
use hypha_customers::CustomerRepo;
use hypha_inventory::{InventoryItemRepo, LocationRepo, StockContext};
use hypha_markets::{CatalogContext, CatalogRepo, Market, MarketRepo, PriceListRepo};
use hypha_pricing::PricingContext;
use hypha_products::{Product, ProductRepo};
use hypha_tax::{TaxClassRepo, TaxContext, TaxRegionRepo};
use thiserror::Error;

use crate::{
    Cart, CartError, CartRepo, LineError, LineStatus, PricedCart, Quantity, check_stock,
    price_cart, price_line,
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
    Line(#[from] LineError),
    #[error(transparent)]
    Money(#[from] MoneyError),
    #[error(transparent)]
    Email(#[from] EmailError),
    #[error(transparent)]
    Country(#[from] CountryError),
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
            CommandError::CustomerNotFound(_) => "customer_not_found",
            CommandError::CartNotFound(_) => "cart_not_found",
            CommandError::MarketNotFound(_) => "market_not_found",
            CommandError::NoMarketForCountry(_) => "no_market_for_country",
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
    type TaxClasses: TaxClassRepo;
    type TaxRegions: TaxRegionRepo;
    type InventoryItems: InventoryItemRepo;
    type Locations: LocationRepo;
    type Customers: CustomerRepo;

    fn carts(&self) -> &Self::Carts;
    fn products(&self) -> &Self::Products;
    fn markets(&self) -> &Self::Markets;
    fn catalogs(&self) -> &Self::Catalogs;
    fn price_lists(&self) -> &Self::PriceLists;
    fn tax_classes(&self) -> &Self::TaxClasses;
    fn tax_regions(&self) -> &Self::TaxRegions;
    fn inventory_items(&self) -> &Self::InventoryItems;
    fn locations(&self) -> &Self::Locations;
    fn customers(&self) -> &Self::Customers;
}

/// Borrowed ports are ports too, so a host can lend out `&Storage` per request.
impl<T: CartPorts + ?Sized> CartPorts for &T {
    type Carts = T::Carts;
    type Products = T::Products;
    type Markets = T::Markets;
    type Catalogs = T::Catalogs;
    type PriceLists = T::PriceLists;
    type TaxClasses = T::TaxClasses;
    type TaxRegions = T::TaxRegions;
    type InventoryItems = T::InventoryItems;
    type Locations = T::Locations;
    type Customers = T::Customers;

    fn carts(&self) -> &Self::Carts {
        (**self).carts()
    }
    fn products(&self) -> &Self::Products {
        (**self).products()
    }
    fn markets(&self) -> &Self::Markets {
        (**self).markets()
    }
    fn catalogs(&self) -> &Self::Catalogs {
        (**self).catalogs()
    }
    fn price_lists(&self) -> &Self::PriceLists {
        (**self).price_lists()
    }
    fn tax_classes(&self) -> &Self::TaxClasses {
        (**self).tax_classes()
    }
    fn tax_regions(&self) -> &Self::TaxRegions {
        (**self).tax_regions()
    }
    fn inventory_items(&self) -> &Self::InventoryItems {
        (**self).inventory_items()
    }
    fn locations(&self) -> &Self::Locations {
        (**self).locations()
    }
    fn customers(&self) -> &Self::Customers {
        (**self).customers()
    }
}

/// Creates an empty cart shipping to `country`, in the market that sells there
/// (like Shopify, the buyer's country picks the market). `country` is an ISO 3166-1
/// alpha-2 code, checked here.
pub async fn create_cart<S: CartPorts, I: IdGenerator>(
    ctx: &Ctx,
    ports: &S,
    ids: &I,
    country: &str,
) -> Result<Cart, CommandError> {
    ctx.require(Scope::CartsWrite)?;

    let country = CountryCode::new(country)?;
    let market = ports
        .markets()
        .get_by_country(country)
        .await?
        .ok_or(CommandError::NoMarketForCountry(country))?;

    let mut cart = Cart::new(CartId::from_uuid(ids.new_uuid()), market.id(), country);
    ports.carts().insert(&mut cart).await?;
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
/// catalog with a price in its currency, and the market's locations must have stock
/// for the line's new quantity (design note 0004, I6). Other lines that went stale
/// don't block it; they come back marked unavailable.
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

    // Only changes the copy in memory; nothing is saved unless every check below passes.
    cart.add_line(product.id(), input.variant, quantity)?;

    let pricing = load_pricing(ports, &cart).await?;
    price_line(&product, input.variant, &pricing)?;

    // The whole line counts: adding 1 to a line of 2 needs 3 in stock. Nothing is
    // reserved; stock is only taken at checkout.
    let stock = load_stock(ports, &cart).await?;
    let line = cart.line(input.variant)?;
    check_stock(&stock, input.variant, line.quantity())?;

    let tax = load_tax(ports, &cart, &pricing.market).await?;
    let cart_products = load_products(ports, &cart).await?;
    let priced = price_cart(&cart, &cart_products, &pricing, &tax, &stock);

    // The strict checks above passed, so if the new line still comes back unavailable,
    // its total (or the cart's with it) doesn't fit: refuse it rather than store it.
    if let Some(LineStatus::Unavailable(reason)) = priced
        .lines
        .iter()
        .find(|l| l.variant == input.variant)
        .map(|l| &l.status)
    {
        return Err(CommandError::Line(reason.clone()));
    }

    // Fails with StaleVersion (409) if another request changed the cart meanwhile,
    // instead of silently dropping that request's line.
    ports.carts().update(&mut cart).await?;
    Ok(priced)
}

/// Sets who to contact about the order this cart becomes; `None` clears it.
pub async fn set_cart_email<S: CartPorts>(
    ctx: &Ctx,
    ports: &S,
    id: CartId,
    email: Option<String>,
) -> Result<Cart, CommandError> {
    ctx.require(Scope::CartsWrite)?;

    let email = email.as_deref().map(Email::new).transpose()?;
    let mut cart = load_cart(ports, id).await?;
    cart.set_email(email)?;
    ports.carts().update(&mut cart).await?;
    Ok(cart)
}

/// Links the cart to an existing customer (taking their email), or unlinks it with
/// `None` (design note 0005, C5). Linking needs `customers:read` as well as `carts:write`.
pub async fn set_cart_customer<S: CartPorts>(
    ctx: &Ctx,
    ports: &S,
    id: CartId,
    customer: Option<CustomerId>,
) -> Result<Cart, CommandError> {
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
    ports.carts().update(&mut cart).await?;
    Ok(cart)
}

async fn load_cart<S: CartPorts>(ports: &S, id: CartId) -> Result<Cart, CommandError> {
    ports
        .carts()
        .get(id)
        .await?
        .ok_or(CommandError::CartNotFound(id))
}

/// The cart at current prices, tax rates and stock. Stale lines are marked unavailable,
/// not errors.
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
    let pricing = load_pricing(ports, &cart).await?;
    let tax = load_tax(ports, &cart, &pricing.market).await?;
    let stock = load_stock(ports, &cart).await?;
    let cart_products = load_products(ports, &cart).await?;

    Ok(price_cart(&cart, &cart_products, &pricing, &tax, &stock))
}

/// Everything needed to price the cart's lines, with a fixed number of queries
/// however many lines there are: membership and fixed prices are fetched in batches.
/// With `load_tax`, `load_stock` and `load_products`, pricing a cart costs 10 queries
/// in total.
///
/// A missing catalog or price list is left out rather than reported here:
/// `resolve_price` turns it into a precise per-line reason.
async fn load_pricing<S: CartPorts>(
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
async fn load_tax<S: CartPorts>(
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
async fn load_stock<S: CartPorts>(ports: &S, cart: &Cart) -> Result<StockContext, CommandError> {
    let variants: Vec<VariantId> = cart.lines().iter().map(|l| l.variant()).collect();
    let items = ports.inventory_items().for_variants(&variants).await?;
    Ok(StockContext {
        items: items.into_iter().map(|i| (i.variant(), i)).collect(),
        locations: ports.locations().serving(cart.market()).await?,
    })
}

/// The cart's products, in one query. Missing (deleted) products are left out;
/// `price_cart` marks their lines unavailable with `ProductNotFound`.
async fn load_products<S: CartPorts>(ports: &S, cart: &Cart) -> Result<Vec<Product>, CommandError> {
    Ok(ports.products().get_many(&product_ids(cart)).await?)
}

/// Each product in the cart once.
fn product_ids(cart: &Cart) -> Vec<ProductId> {
    let mut ids: Vec<ProductId> = cart.lines().iter().map(|l| l.product()).collect();
    ids.sort_by_key(|p| p.as_uuid());
    ids.dedup();
    ids
}
