//! The search index's own commands (design note 0019, sections 6 and 7): the
//! merchant's settings, a full rebuild, and how far behind the index is. Searching a
//! market is `hypha_search::search` (it resolves the market).

use hypha_core::{AuthError, Ctx, ErrorCode, RepoError, Scope};
use thiserror::Error;

use crate::{
    FacetSetting, HasSearchQueue, HasSearchSettings, IndexError, SearchError, SearchIndex,
    SearchQueue, SearchSettings, SearchSettingsRepo, Target,
};

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum CommandError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Search(#[from] SearchError),
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error(transparent)]
    Index(#[from] IndexError),
}

impl ErrorCode for CommandError {
    fn code(&self) -> &'static str {
        match self {
            CommandError::Auth(e) => e.code(),
            CommandError::Search(e) => e.code(),
            CommandError::Repo(e) => e.code(),
            CommandError::Index(e) => e.code(),
        }
    }
}

/// Queues a rebuild of everything: after a new engine or a lost index.
pub async fn queue_all<S: HasSearchQueue>(ports: &S) -> Result<(), RepoError> {
    ports.search_queue().mark(&[Target::All]).await
}

/// The store's settings, or the defaults when none are saved.
pub async fn get_search_settings<S: HasSearchSettings>(
    ctx: &Ctx,
    ports: &S,
) -> Result<SearchSettings, CommandError> {
    ctx.require(Scope::ProductsRead)?;
    Ok(ports.search_settings().get().await?.unwrap_or_default())
}

/// Changes only what's `Some`, like a JSON Merge Patch.
#[derive(Debug, Clone, Default)]
pub struct UpdateSearchSettings {
    pub split_by: Option<Vec<String>>,
    /// Facet keys as strings (`option.color`) and labels.
    pub facets: Option<Vec<(String, Option<String>)>>,
}

/// Changes the settings (search settings are how products are presented, so
/// `products:write`). Changing the splits rebuilds every document, through the
/// `search_settings.changed` event.
pub async fn update_search_settings<S: HasSearchSettings>(
    ctx: &Ctx,
    ports: &S,
    input: UpdateSearchSettings,
) -> Result<SearchSettings, CommandError> {
    ctx.require(Scope::ProductsWrite)?;
    let stored = ports.search_settings().get().await?;
    let is_new = stored.is_none();
    let mut settings = stored.unwrap_or_default();
    if let Some(names) = input.split_by {
        settings.set_split_by(names)?;
    }
    if let Some(facets) = input.facets {
        let facets = facets
            .into_iter()
            .map(|(key, label)| {
                Ok(FacetSetting {
                    key: key.parse().map_err(SearchError::UnknownFacet)?,
                    label,
                })
            })
            .collect::<Result<Vec<_>, SearchError>>()?;
        settings.set_facets(facets)?;
    }
    if is_new {
        ports.search_settings().insert(&mut settings).await?;
    } else {
        ports.search_settings().update(&mut settings).await?;
    }
    Ok(settings)
}

/// Queues a rebuild of every document.
pub async fn reindex<S: HasSearchQueue>(ctx: &Ctx, ports: &S) -> Result<(), CommandError> {
    ctx.require(Scope::ProductsWrite)?;
    Ok(queue_all(ports).await?)
}

/// How the index is doing: what's waiting to be rebuilt, and how much it holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchStatus {
    pub queued: u64,
    pub documents: u64,
}

pub async fn search_status<S: HasSearchQueue, I: SearchIndex>(
    ctx: &Ctx,
    ports: &S,
    index: &I,
) -> Result<SearchStatus, CommandError> {
    ctx.require(Scope::ProductsRead)?;
    Ok(SearchStatus {
        queued: ports.search_queue().queued().await?,
        documents: index.count().await?,
    })
}
