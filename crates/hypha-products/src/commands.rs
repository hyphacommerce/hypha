use hypha_core::{
    AuthError, Ctx, ErrorCode, IdGenerator, Keyset, Money, MoneyError, OptionValueId, Page,
    PageRequest, Price, ProductId, RepoError, Scope, VariantId, Weight, report_duplicate,
};
use thiserror::Error;

use crate::{
    Handle, NewVariant, OptionDraft, Product, ProductError, ProductFilter, ProductFilterInput,
    ProductRepo, ProductSort, ProductStatus, ProductSummary, VariantDraft, VariantPatch,
    repo::keys,
};

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum CommandError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Product(#[from] ProductError),
    #[error(transparent)]
    Money(#[from] MoneyError),
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error("handle {handle} is already taken by product {product}")]
    HandleTaken { handle: Handle, product: ProductId },
    #[error("product {0} not found")]
    ProductNotFound(ProductId),
    #[error("SKU {sku:?} is already used by product {product}")]
    SkuTaken { sku: String, product: ProductId },
}

impl ErrorCode for CommandError {
    fn code(&self) -> &'static str {
        match self {
            CommandError::Auth(e) => e.code(),
            CommandError::Product(e) => e.code(),
            CommandError::Money(e) => e.code(),
            CommandError::Repo(e) => e.code(),
            CommandError::HandleTaken { .. } => "handle_taken",
            CommandError::ProductNotFound(_) => "product_not_found",
            CommandError::SkuTaken { .. } => "sku_taken",
        }
    }
}

pub struct CreateSimpleProduct {
    pub handle: String,
    pub title: String,
    pub price: Money,
}

pub async fn create_simple_product<R: ProductRepo, I: IdGenerator>(
    ctx: &Ctx,
    repo: &R,
    ids: &I,
    input: CreateSimpleProduct,
) -> Result<Product, CommandError> {
    ctx.require(Scope::ProductsWrite)?;

    let handle = Handle::new(&input.handle).map_err(ProductError::from)?;
    let price = Price::new(input.price)?;

    // All input is checked before anything is read.
    let mut product = Product::new_simple(
        ProductId::from_uuid(ids.new_uuid()),
        handle,
        input.title,
        VariantId::from_uuid(ids.new_uuid()),
        price,
    )?;

    // Precise error in the normal case; the repo's unique key catches a concurrent insert.
    if let Some(taken) = handle_taken(repo, product.handle()).await? {
        return Err(taken);
    }
    let written = repo.insert(&mut product).await;
    report_duplicate(written, keys::HANDLE, handle_taken(repo, product.handle())).await?;
    Ok(product)
}

/// `HandleTaken` naming the product that has `handle`, if one does.
async fn handle_taken<R: ProductRepo>(
    repo: &R,
    handle: &Handle,
) -> Result<Option<CommandError>, CommandError> {
    Ok(repo
        .get_by_handle(handle)
        .await?
        .map(|existing| CommandError::HandleTaken {
            handle: handle.clone(),
            product: existing.id(),
        }))
}

/// A list's filters and order as a request gives them; `list_products` checks them.
#[derive(Debug, Default)]
pub struct ListProducts {
    pub filter: ProductFilterInput,
    pub sort: ProductSort,
}

/// A page of product summaries in `input.sort`'s order, narrowed by its filter (design
/// notes 0012, 0019, 0024). `page.after`'s key must fit the sort (`ProductSort::fits`;
/// hosts check cursors).
pub async fn list_products<R: ProductRepo>(
    ctx: &Ctx,
    repo: &R,
    input: ListProducts,
    page: PageRequest<Keyset<ProductId>>,
) -> Result<Page<ProductSummary>, CommandError> {
    ctx.require(Scope::ProductsRead)?;
    let filter = ProductFilter::new(input.filter)?;
    Ok(repo.list(&filter, input.sort, page).await?)
}

pub async fn get_product<R: ProductRepo>(
    ctx: &Ctx,
    repo: &R,
    id: ProductId,
) -> Result<Product, CommandError> {
    ctx.require(Scope::ProductsRead)?;
    load(repo, id).await
}

/// The products of `ids` that exist, in no particular order: for lists that hold
/// product IDs (a catalog's products, design note 0021).
pub async fn get_products<R: ProductRepo>(
    ctx: &Ctx,
    repo: &R,
    ids: &[ProductId],
) -> Result<Vec<Product>, CommandError> {
    ctx.require(Scope::ProductsRead)?;
    Ok(repo.get_many(ids).await?)
}

/// The products owning any of `variants`, in no particular order: for lists that hold
/// variant IDs (a price list's fixed prices).
pub async fn products_of_variants<R: ProductRepo>(
    ctx: &Ctx,
    repo: &R,
    variants: &[VariantId],
) -> Result<Vec<Product>, CommandError> {
    ctx.require(Scope::ProductsRead)?;
    let ids = repo.ids_of_variants(variants).await?;
    Ok(repo.get_many(&ids).await?)
}

/// Moves a product to `status`, following the allowed transitions
/// (draft ↔ active, active → archived, archived → draft).
pub async fn set_product_status<R: ProductRepo>(
    ctx: &Ctx,
    repo: &R,
    id: ProductId,
    status: ProductStatus,
) -> Result<Product, CommandError> {
    ctx.require(Scope::ProductsWrite)?;

    let mut product = load(repo, id).await?;
    product.set_status(status)?;
    repo.update(&mut product).await?;
    Ok(product)
}

/// Changes only the fields that are `Some`, like a JSON Merge Patch.
#[derive(Debug, Default)]
pub struct UpdateProduct {
    pub title: Option<String>,
    pub handle: Option<String>,
    /// `Some(None)` clears it (design note 0019, section 2).
    pub description: Option<Option<String>>,
    pub vendor: Option<Option<String>>,
    pub product_type: Option<Option<String>>,
    /// Replaces them all.
    pub tags: Option<Vec<String>>,
}

/// Changes a product's title, handle or content (design notes 0014 and 0019).
pub async fn update_product<R: ProductRepo>(
    ctx: &Ctx,
    repo: &R,
    id: ProductId,
    input: UpdateProduct,
) -> Result<Product, CommandError> {
    ctx.require(Scope::ProductsWrite)?;
    let handle = input
        .handle
        .as_deref()
        .map(Handle::new)
        .transpose()
        .map_err(ProductError::from)?;

    let mut product = load(repo, id).await?;
    if let Some(title) = &input.title {
        product.rename(title)?;
    }
    if let Some(description) = &input.description {
        product.set_description(description.as_deref())?;
    }
    if let Some(vendor) = input.vendor {
        product.set_vendor(vendor)?;
    }
    if let Some(product_type) = input.product_type {
        product.set_product_type(product_type)?;
    }
    if let Some(tags) = input.tags {
        product.set_tags(tags)?;
    }
    if let Some(handle) = handle
        && &handle != product.handle()
    {
        if let Some(taken) = handle_taken(repo, &handle).await? {
            return Err(taken);
        }
        product.set_handle(handle);
    }
    save(repo, &mut product, &[]).await?;
    Ok(product)
}

pub struct SetProductOptions {
    /// The options in order: existing ones by ID, new ones without.
    pub options: Vec<OptionDraft>,
    /// Variants to remove in the same write: those using a value being removed.
    pub remove_variants: Vec<VariantId>,
}

/// Replaces a product's options and values (design note 0014, section 3; the rules are
/// `Product::set_options`'s).
pub async fn set_product_options<R: ProductRepo, I: IdGenerator>(
    ctx: &Ctx,
    repo: &R,
    ids: &I,
    id: ProductId,
    input: SetProductOptions,
) -> Result<Product, CommandError> {
    ctx.require(Scope::ProductsWrite)?;
    let mut product = load(repo, id).await?;
    product.set_options(input.options, &input.remove_variants, ids)?;
    save(repo, &mut product, &[]).await?;
    Ok(product)
}

pub struct AddVariant {
    pub sku: Option<String>,
    pub price: Money,
    /// One value per option, in option order.
    pub selections: Vec<OptionValueId>,
    pub weight: Option<Weight>,
    /// `true` unless the variant is never shipped (design note 0015, SH11).
    pub requires_shipping: bool,
}

/// Adds one variant: the combination a merchant sells, never all of them (E5).
pub async fn add_product_variant<R: ProductRepo, I: IdGenerator>(
    ctx: &Ctx,
    repo: &R,
    ids: &I,
    id: ProductId,
    input: AddVariant,
) -> Result<Product, CommandError> {
    ctx.require(Scope::ProductsWrite)?;
    let price = Price::new(input.price)?;
    let mut product = load(repo, id).await?;
    let variant = VariantId::from_uuid(ids.new_uuid());
    product.add_variant(NewVariant {
        id: variant,
        sku: input.sku,
        price,
        selections: input.selections,
    })?;
    product.update_variant(
        variant,
        VariantPatch {
            weight: Some(input.weight),
            requires_shipping: Some(input.requires_shipping),
            ..VariantPatch::default()
        },
    )?;
    let sku = product
        .variant(variant)
        .and_then(|v| v.sku())
        .map(str::to_string);
    save(repo, &mut product, sku.as_slice()).await?;
    Ok(product)
}

/// Changes only the fields that are `Some`; `sku: Some(None)` clears the SKU.
#[derive(Debug, Default)]
pub struct UpdateVariant {
    pub sku: Option<Option<String>>,
    pub price: Option<Money>,
    pub selections: Option<Vec<OptionValueId>>,
    /// `Some(None)` clears the weight.
    pub weight: Option<Option<Weight>>,
    pub requires_shipping: Option<bool>,
}

pub async fn update_product_variant<R: ProductRepo>(
    ctx: &Ctx,
    repo: &R,
    id: ProductId,
    variant: VariantId,
    input: UpdateVariant,
) -> Result<Product, CommandError> {
    ctx.require(Scope::ProductsWrite)?;
    let price = input.price.map(Price::new).transpose()?;
    let mut product = load(repo, id).await?;
    let sku_before = product
        .variant(variant)
        .and_then(|v| v.sku())
        .map(str::to_string);
    product.update_variant(
        variant,
        VariantPatch {
            sku: input.sku,
            price,
            selections: input.selections,
            weight: input.weight,
            requires_shipping: input.requires_shipping,
        },
    )?;
    let sku = product
        .variant(variant)
        .and_then(|v| v.sku())
        .map(str::to_string);
    // Only a SKU this change gave the variant needs checking against other products.
    let changed: Vec<String> = sku
        .into_iter()
        .filter(|sku| Some(sku) != sku_before.as_ref())
        .collect();
    save(repo, &mut product, &changed).await?;
    Ok(product)
}

/// A variant in a whole list (`set_product_variants`): an existing one by `id`, or a new
/// one without. Every field is given, as in a `PUT`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetVariant {
    pub id: Option<VariantId>,
    pub sku: Option<String>,
    pub price: Money,
    pub selections: Vec<OptionValueId>,
    pub weight: Option<Weight>,
    pub requires_shipping: bool,
}

/// Sets a product's whole variant list in one write (design note 0021, Q1): listed
/// variants with an ID are updated, those without are added, and stored ones left out
/// are removed, all or nothing, with one set of events. The admin's "Create all
/// variants" sends its combinations this way: nothing here expands options, and
/// `Product::MAX_VARIANTS` bounds the list.
pub async fn set_product_variants<R: ProductRepo, I: IdGenerator>(
    ctx: &Ctx,
    repo: &R,
    ids: &I,
    id: ProductId,
    variants: Vec<SetVariant>,
) -> Result<Product, CommandError> {
    ctx.require(Scope::ProductsWrite)?;
    let drafts = variants
        .into_iter()
        .map(|v| {
            Ok(VariantDraft {
                id: v.id,
                sku: v.sku,
                price: Price::new(v.price)?,
                selections: v.selections,
                weight: v.weight,
                requires_shipping: v.requires_shipping,
            })
        })
        .collect::<Result<Vec<_>, CommandError>>()?;
    let mut product = load(repo, id).await?;
    let skus_before: Vec<String> = product
        .variants()
        .iter()
        .filter_map(|v| v.sku().map(str::to_string))
        .collect();
    product.set_variants(drafts, ids)?;
    let new_skus: Vec<String> = product
        .variants()
        .iter()
        .filter_map(|v| v.sku())
        .filter(|sku| !skus_before.iter().any(|before| before == sku))
        .map(str::to_string)
        .collect();
    save(repo, &mut product, &new_skus).await?;
    Ok(product)
}

/// Removes a variant. Price lists and inventory keep its ID until the outbox cleans
/// them up (design note 0014, section 4); carts show it as unavailable.
pub async fn remove_product_variant<R: ProductRepo>(
    ctx: &Ctx,
    repo: &R,
    id: ProductId,
    variant: VariantId,
) -> Result<Product, CommandError> {
    ctx.require(Scope::ProductsWrite)?;
    let mut product = load(repo, id).await?;
    product.remove_variant(variant)?;
    save(repo, &mut product, &[]).await?;
    Ok(product)
}

async fn load<R: ProductRepo>(repo: &R, id: ProductId) -> Result<Product, CommandError> {
    repo.get(id).await?.ok_or(CommandError::ProductNotFound(id))
}

/// Writes a changed product. `new_skus` are SKUs this change gave variants: they're
/// checked against other products first, and a concurrent write that takes one first
/// reports the same `SkuTaken`, like a taken handle.
async fn save<R: ProductRepo>(
    repo: &R,
    product: &mut Product,
    new_skus: &[String],
) -> Result<(), CommandError> {
    if let Some(taken) = first_sku_taken(repo, product.id(), new_skus).await? {
        return Err(taken);
    }
    match repo.update(product).await {
        Err(RepoError::Duplicate(key)) if key == keys::VARIANT_SKU && !new_skus.is_empty() => {
            let taken = first_sku_taken(repo, product.id(), new_skus).await?;
            Err(taken.unwrap_or(RepoError::Duplicate(key).into()))
        }
        written => {
            report_duplicate(written, keys::HANDLE, handle_taken(repo, product.handle())).await
        }
    }
}

/// `SkuTaken` for the first of `skus` another product has.
async fn first_sku_taken<R: ProductRepo>(
    repo: &R,
    own: ProductId,
    skus: &[String],
) -> Result<Option<CommandError>, CommandError> {
    for sku in skus {
        if let Some(taken) = sku_taken(repo, own, sku).await? {
            return Ok(Some(taken));
        }
    }
    Ok(None)
}

/// `SkuTaken` naming the other product that has `sku`, if one does.
async fn sku_taken<R: ProductRepo>(
    repo: &R,
    own: ProductId,
    sku: &str,
) -> Result<Option<CommandError>, CommandError> {
    Ok(repo
        .get_by_sku(sku)
        .await?
        .filter(|other| other.id() != own)
        .map(|other| CommandError::SkuTaken {
            sku: sku.to_string(),
            product: other.id(),
        }))
}
