use hypha_core::{
    AuthError, Ctx, ErrorCode, IdGenerator, Money, MoneyError, Price, ProductId, RepoError, Scope,
    VariantId,
};
use thiserror::Error;

use crate::{Handle, Product, ProductError, ProductRepo, ProductStatus};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CommandError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Product(#[from] ProductError),
    #[error(transparent)]
    Money(#[from] MoneyError),
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error("handle {0} is already taken")]
    HandleTaken(Handle),
    #[error("product {0} not found")]
    NotFound(ProductId),
}

impl ErrorCode for CommandError {
    fn code(&self) -> &'static str {
        match self {
            CommandError::Auth(e) => e.code(),
            CommandError::Product(e) => e.code(),
            CommandError::Money(e) => e.code(),
            CommandError::Repo(e) => e.code(),
            CommandError::HandleTaken(_) => "handle_taken",
            CommandError::NotFound(_) => "product_not_found",
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

    if repo.get_by_handle(&handle).await?.is_some() {
        return Err(CommandError::HandleTaken(handle));
    }

    let product = Product::new_simple(
        ProductId::from_uuid(ids.new_uuid()),
        handle,
        input.title,
        VariantId::from_uuid(ids.new_uuid()),
        price,
    );

    repo.save(&product).await?;
    Ok(product)
}

pub async fn get_product<R: ProductRepo>(
    ctx: &Ctx,
    repo: &R,
    id: ProductId,
) -> Result<Product, CommandError> {
    ctx.require(Scope::ProductsRead)?;
    repo.get(id).await?.ok_or(CommandError::NotFound(id))
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

    let mut product = repo.get(id).await?.ok_or(CommandError::NotFound(id))?;
    product.set_status(status)?;
    repo.save(&product).await?;
    Ok(product)
}
