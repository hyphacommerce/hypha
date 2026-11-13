use hypha_core::{
    Address, AddressError, AddressParts, AuthError, Clock, Ctx, Currency, CurrencyError, Email,
    EmailError, ErrorCode, IdGenerator, Language, LanguageError, MarketId, RepoError, Scope,
    StoreId, Timezone, TimezoneError,
};
use thiserror::Error;

use crate::{SetupStep, Store, StoreError, StoreLanguage, StoreRepo};

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
    #[error(transparent)]
    Currency(#[from] CurrencyError),
    #[error("store {0} not found")]
    StoreNotFound(StoreId),
    /// Costs are recorded in the base currency: changing it would misread them.
    #[error("the base currency can't change once a cost is recorded in it")]
    CurrencyLocked,
    /// Markets still offer languages the store would stop offering (removed, unpublished,
    /// or the default one replaced): each such market with those languages.
    #[error("{} market(s) still offer a language the store would no longer offer", .0.len())]
    LanguageInUse(Vec<MarketLanguages>),
    /// One store per deployment (model A) found more than one.
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
            CommandError::Currency(e) => e.code(),
            CommandError::StoreNotFound(_) => "store_not_found",
            CommandError::CurrencyLocked => "store_currency_locked",
            CommandError::LanguageInUse(_) => "language_in_use",
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
    /// The languages beside the default one, replacing the list.
    pub languages: Option<Vec<LanguageInput>>,
    /// Set by the host, not the caller: the languages each market offers by name, which
    /// the store must keep offering (its default, or published).
    pub market_languages: Vec<MarketLanguages>,
    /// An ISO 4217 code: the base currency costs are kept in.
    pub currency: Option<Option<String>>,
    /// Set by the host, not the caller: whether a cost was ever recorded, which fixes
    /// the currency.
    pub currency_locked: bool,
    /// The setup guide's step the merchant is on.
    pub setup_step: Option<Option<String>>,
    /// `true` marks the guide done (now, the first time); `false` shows it again.
    pub setup_finished: Option<bool>,
}

/// One of the store's other languages, unvalidated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageInput {
    /// A BCP 47 tag.
    pub language: String,
    pub published: bool,
}

/// Languages a market names: what it offers (the host's input), or what it would be left
/// offering without the store's say (`CommandError::LanguageInUse`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarketLanguages {
    pub market: MarketId,
    pub languages: Vec<Language>,
}

impl UpdateStore {
    /// Whether the caller asked for no change; what the host adds doesn't count.
    fn is_empty(&self) -> bool {
        *self
            == Self {
                currency_locked: self.currency_locked,
                market_languages: self.market_languages.clone(),
                ..Self::default()
            }
    }
}

/// The markets naming a language the store offered `before` and no longer does, each
/// with those languages. A language it didn't offer before isn't this change's doing,
/// so a market out of step already doesn't stop the store's other changes.
fn languages_in_use(
    before: &[Language],
    store: &Store,
    markets: Vec<MarketLanguages>,
) -> Vec<MarketLanguages> {
    let offered = store.offered_languages();
    markets
        .into_iter()
        .filter_map(|mut m| {
            m.languages
                .retain(|l| before.contains(l) && !offered.contains(l));
            (!m.languages.is_empty()).then_some(m)
        })
        .collect()
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
    let offered_before = store.offered_languages();
    let default_language = input
        .default_language
        .as_deref()
        .map(Language::new)
        .transpose()?;
    match (default_language, input.languages) {
        // Together, so the old default and one of the others can swap in one request.
        (default, Some(languages)) => {
            let default = default.unwrap_or_else(|| store.default_language().clone());
            let languages = languages
                .into_iter()
                .map(|l| {
                    Ok(StoreLanguage {
                        language: Language::new(&l.language)?,
                        published: l.published,
                    })
                })
                .collect::<Result<Vec<_>, LanguageError>>()?;
            store.set_languages(default, languages)?;
        }
        (Some(default), None) => store.set_default_language(default)?,
        (None, None) => {}
    }
    let in_use = languages_in_use(&offered_before, &store, input.market_languages);
    if !in_use.is_empty() {
        return Err(CommandError::LanguageInUse(in_use));
    }
    if let Some(currency) = &input.currency {
        let currency = currency.as_deref().map(Currency::new).transpose()?;
        if input.currency_locked && currency != store.currency() {
            return Err(CommandError::CurrencyLocked);
        }
        store.set_currency(currency);
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

/// Startup rule for one store per deployment (model A): returns the deployment's store,
/// creating it with `default_name` on first start. More than one store means the deployment
/// isn't model A: an error, not something to guess about.
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
