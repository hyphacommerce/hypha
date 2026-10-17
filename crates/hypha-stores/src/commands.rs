use hypha_core::{
    Address, AddressError, AddressParts, AuthError, Clock, Ctx, Email, EmailError, ErrorCode,
    IdGenerator, Language, LanguageError, RepoError, Scope, StoreId, Timezone, TimezoneError,
};
use thiserror::Error;

use crate::{SetupStep, Store, StoreError, StoreRepo};

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum CommandError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Email(#[from] EmailError),
    #[error(transparent)]
    Address(#[from] AddressError),
    #[error(transparent)]
    Timezone(#[from] TimezoneError),
    #[error(transparent)]
    Language(#[from] LanguageError),
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
            CommandError::Email(e) => e.code(),
            CommandError::Address(e) => e.code(),
            CommandError::Timezone(e) => e.code(),
            CommandError::Language(e) => e.code(),
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

/// The store's details to change, unvalidated: `None` keeps a field as it is, and
/// `Some(None)` clears an optional one (JSON Merge Patch).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UpdateStore {
    pub name: Option<String>,
    pub contact_email: Option<Option<String>>,
    pub description: Option<Option<String>>,
    pub legal_name: Option<Option<String>>,
    pub legal_address: Option<Option<AddressParts>>,
    /// An IANA name (`Europe/Belgrade`).
    pub timezone: Option<Option<String>>,
    /// A BCP 47 tag (`en`, `sr-Latn`).
    pub default_language: Option<String>,
    /// The setup guide's step the merchant is on (design note 0024, AU11).
    pub setup_step: Option<Option<String>>,
    /// `true` marks the guide done (now, the first time); `false` shows it again.
    pub setup_finished: Option<bool>,
}

impl UpdateStore {
    fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Changes the store's details. Needs `stores:write` even when nothing changes, so the
/// scope a caller needs doesn't depend on the body it sends; with nothing to change it
/// writes nothing and returns the store.
pub async fn update_store<R: StoreRepo>(
    ctx: &Ctx,
    repo: &R,
    clock: &impl Clock,
    id: StoreId,
    input: UpdateStore,
) -> Result<Store, CommandError> {
    ctx.require(Scope::StoresWrite)?;
    let mut store = repo.get(id).await?.ok_or(CommandError::StoreNotFound(id))?;
    if input.is_empty() {
        return Ok(store);
    }
    if let Some(name) = &input.name {
        store.rename(name)?;
    }
    if let Some(email) = input.contact_email {
        store.set_contact_email(email.as_deref().map(Email::new).transpose()?);
    }
    if let Some(description) = &input.description {
        store.set_description(description.as_deref())?;
    }
    if let Some(legal_name) = &input.legal_name {
        store.set_legal_name(legal_name.as_deref())?;
    }
    if let Some(address) = input.legal_address {
        store.set_legal_address(address.map(Address::new).transpose()?);
    }
    if let Some(timezone) = input.timezone {
        store.set_timezone(timezone.as_deref().map(Timezone::new).transpose()?);
    }
    if let Some(language) = &input.default_language {
        store.set_default_language(Language::new(language)?);
    }
    if let Some(step) = &input.setup_step {
        store.set_setup_step(step.as_deref().map(SetupStep::new).transpose()?);
    }
    match input.setup_finished {
        Some(true) => store.finish_setup(clock.now()),
        Some(false) => store.reopen_setup(),
        None => {}
    }
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
