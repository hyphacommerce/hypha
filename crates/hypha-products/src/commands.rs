use hypha_core::{
    AuthError, Ctx, ErrorCode, IdGenerator, Money, MoneyError, Price, ProductId, RepoError, Scope,
    VariantId, report_duplicate,
};
use thiserror::Error;

use crate::{Handle, Product, ProductError, ProductRepo, ProductStatus, repo::keys};

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

    let handle = Handle::new(&input.handle)?;
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

pub async fn get_product<R: ProductRepo>(
    ctx: &Ctx,
    repo: &R,
    id: ProductId,
) -> Result<Product, CommandError> {
    ctx.require(Scope::ProductsRead)?;
    repo.get(id).await?.ok_or(CommandError::ProductNotFound(id))
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

    let mut product = repo
        .get(id)
        .await?
        .ok_or(CommandError::ProductNotFound(id))?;
    product.set_status(status)?;
    repo.update(&mut product).await?;
    Ok(product)
}
