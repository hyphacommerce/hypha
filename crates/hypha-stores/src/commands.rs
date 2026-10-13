use hypha_core::{AuthError, Ctx, ErrorCode, IdGenerator, RepoError, Scope, StoreId};
use thiserror::Error;

use crate::{Store, StoreError, StoreRepo};

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum CommandError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("store {0} not found")]
    StoreNotFound(StoreId),
    /// One store per deployment (design note 0006, model A) found more than one.
    #[error("expected exactly one store in this deployment, found {0}")]
    MultipleStores(usize),
}

impl ErrorCode for CommandError {
    fn code(&self) -> &'static str {
        match self {
            CommandError::Auth(e) => e.code(),
            CommandError::Repo(e) => e.code(),
            CommandError::Store(e) => e.code(),
            CommandError::StoreNotFound(_) => "store_not_found",
            CommandError::MultipleStores(_) => "multiple_stores",
        }
    }
}

pub async fn create_store<R: StoreRepo, I: IdGenerator>(
    ctx: &Ctx,
    repo: &R,
    ids: &I,
    name: &str,
) -> Result<Store, CommandError> {
    ctx.require(Scope::StoresWrite)?;
    let mut store = Store::new(StoreId::from_uuid(ids.new_uuid()), name)?;
    repo.insert(&mut store).await?;
    Ok(store)
}

pub async fn get_store<R: StoreRepo>(
    ctx: &Ctx,
    repo: &R,
    id: StoreId,
) -> Result<Store, CommandError> {
    ctx.require(Scope::StoresRead)?;
    repo.get(id).await?.ok_or(CommandError::StoreNotFound(id))
}

pub async fn rename_store<R: StoreRepo>(
    ctx: &Ctx,
    repo: &R,
    id: StoreId,
    name: &str,
) -> Result<Store, CommandError> {
    ctx.require(Scope::StoresWrite)?;
    let mut store = repo.get(id).await?.ok_or(CommandError::StoreNotFound(id))?;
    store.rename(name)?;
    repo.update(&mut store).await?;
    Ok(store)
}

/// Startup rule for one store per deployment (design note 0006, model A): returns the
/// deployment's store, creating it with `default_name` on first start. More than one
/// store means the deployment isn't model A: an error, not something to guess about.
pub async fn ensure_single_store<R: StoreRepo, I: IdGenerator>(
    ctx: &Ctx,
    repo: &R,
    ids: &I,
    default_name: &str,
) -> Result<Store, CommandError> {
    let mut stores = repo.list_all().await?;
    match stores.len() {
        0 => create_store(ctx, repo, ids, default_name).await,
        1 => Ok(stores.remove(0)),
        n => Err(CommandError::MultipleStores(n)),
    }
}
