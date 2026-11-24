use std::collections::BTreeMap;

use hypha_core::{
    AuthError, Ctx, ErrorCode, Language, LanguageError, Page, PageRequest, RepoError, Scope,
};

use crate::{
    catalog::{ResourceKind, ResourceRef},
    error::TranslationError,
    repo::{
        HasTranslations, TranslationFilter, TranslationKey, TranslationRepo, TranslationSourceRepo,
        keys,
    },
    source::Source,
    translation::{FieldInput, Translation},
    worklist::{
        Inheritance, ProgressLanguage, TranslationProgress, TranslationState, WorkItem,
        WorklistFilter, source_digests,
    },
};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CommandError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error(transparent)]
    Translation(#[from] TranslationError),
    #[error(transparent)]
    Language(#[from] LanguageError),
    #[error("no {} {}", .0.kind, .0.id_string())]
    ResourceNotFound(ResourceRef),
    #[error("{} {} has no translation into {language}", resource.kind, resource.id_string())]
    NotFound {
        resource: ResourceRef,
        language: Language,
    },
    #[error("{0} is the store's default language: its text is the content itself")]
    DefaultLanguage(Language),
    #[error("{0} isn't one of the store's languages")]
    LanguageNotListed(Language),
}

impl ErrorCode for CommandError {
    fn code(&self) -> &'static str {
        match self {
            CommandError::Auth(e) => e.code(),
            CommandError::Repo(e) => e.code(),
            CommandError::Translation(e) => e.code(),
            CommandError::Language(e) => e.code(),
            CommandError::ResourceNotFound(_) => "resource_not_found",
            CommandError::NotFound { .. } => "translation_not_found",
            CommandError::DefaultLanguage(_) => "translation_into_default_language",
            CommandError::LanguageNotListed(_) => "language_not_listed",
        }
    }
}

/// The store's languages, as plain values: the leaf can't see the store.
#[derive(Debug, Clone, Copy)]
pub struct StoreLanguages<'a> {
    /// The language the content itself is in.
    pub default: &'a Language,
    /// The other languages on its list, published or not.
    pub others: &'a [Language],
}

/// Whether the caller may read `kind`'s translations: `translations:read` and what
/// reading the resource takes. Hosts ask before reading the resource for its source.
pub fn require_read(ctx: &Ctx, kind: ResourceKind) -> Result<(), AuthError> {
    ctx.require(Scope::TranslationsRead)?;
    kind.read_scopes().iter().try_for_each(|s| ctx.require(*s))
}

/// Whether the caller may write `kind`'s translations: `translations:write` and what
/// reading the resource takes.
pub fn require_write(ctx: &Ctx, kind: ResourceKind) -> Result<(), AuthError> {
    ctx.require(Scope::TranslationsWrite)?;
    kind.read_scopes().iter().try_for_each(|s| ctx.require(*s))
}

/// Every translation of a resource, by language tag.
pub async fn get_translations<S: HasTranslations>(
    ctx: &Ctx,
    ports: &S,
    resource: ResourceRef,
) -> Result<Vec<Translation>, CommandError> {
    require_read(ctx, resource.kind)?;
    Ok(ports.translations().for_resource(resource).await?)
}

/// The kinds a list shows the caller: `kind` alone (refused without its read scopes), or
/// every kind whose resources they read, in the catalog's order.
fn readable_kinds(ctx: &Ctx, kind: Option<ResourceKind>) -> Result<Vec<ResourceKind>, AuthError> {
    ctx.require(Scope::TranslationsRead)?;
    Ok(match kind {
        Some(kind) => {
            require_read(ctx, kind)?;
            vec![kind]
        }
        None => ResourceKind::ALL
            .into_iter()
            .filter(|kind| require_read(ctx, *kind).is_ok())
            .collect(),
    })
}

/// A page of the resources with text to translate, by their state in `language`
/// (`state`, or every state): what's left to translate, for the kinds the caller may read
/// (`kind` alone, or every kind whose resources they read). In the catalog's order of
/// kinds, then by resource ID. Each item counts what a regional language inherits from
/// its base among the store's `languages`.
pub async fn translation_worklist<S: HasTranslations>(
    ctx: &Ctx,
    ports: &S,
    kind: Option<ResourceKind>,
    language: Language,
    languages: StoreLanguages<'_>,
    state: Option<TranslationState>,
    page: PageRequest<ResourceRef>,
) -> Result<Page<WorkItem>, CommandError> {
    let filter = WorklistFilter {
        kinds: readable_kinds(ctx, kind)?,
        inherits: Inheritance::of(&language, languages.default, languages.others),
        language,
        state,
    };
    Ok(ports.translations().worklist(&filter, page).await?)
}

/// How far each of the store's other languages covers the text of the kinds the caller
/// may read (`kind` alone, or every kind whose resources they read), in fields, a
/// regional one counting what it inherits from its base, and how many of those kinds'
/// resources are yet to be counted.
pub async fn translation_progress<S: HasTranslations>(
    ctx: &Ctx,
    ports: &S,
    kind: Option<ResourceKind>,
    languages: StoreLanguages<'_>,
) -> Result<TranslationProgress, CommandError> {
    let kinds = readable_kinds(ctx, kind)?;
    let counted: Vec<ProgressLanguage> = languages
        .others
        .iter()
        .map(|language| ProgressLanguage {
            language: language.clone(),
            inherits: Inheritance::of(language, languages.default, languages.others),
        })
        .collect();
    let repo = ports.translations();
    Ok(TranslationProgress {
        languages: repo.progress(&kinds, &counted).await?,
        pending: repo.pending(&kinds).await?,
    })
}

/// Writes `resource`'s digests from `source`, the resource as the host read it now
/// (`None`: it's gone, and its digests go). Idempotent: what's written is what the
/// resource is when it runs. Not a command: the host's handler and job call it.
pub async fn sync_sources<S: HasTranslations>(
    ports: &S,
    resource: ResourceRef,
    source: Option<&Source>,
) -> Result<(), RepoError> {
    let digests = source.map(source_digests).unwrap_or_default();
    ports
        .translations()
        .replace_sources(resource, &digests)
        .await
}

/// A page of the store's translations, of the kinds the caller may read: `kind` alone,
/// or every kind whose resources they read.
pub async fn list_translations<S: HasTranslations>(
    ctx: &Ctx,
    ports: &S,
    kind: Option<ResourceKind>,
    language: Option<Language>,
    page: PageRequest<TranslationKey>,
) -> Result<Page<Translation>, CommandError> {
    let filter = TranslationFilter {
        kinds: Some(readable_kinds(ctx, kind)?),
        language,
    };
    Ok(ports.translations().list(&filter, page).await?)
}

/// How many times a write starts over after another write of the same translation got
/// in first.
const ATTEMPTS: u32 = 3;

/// Sets a resource's translation into `language` to `fields`, replacing what it had: a
/// field left out or blank is removed, and a translation left with none is deleted
/// (`None`). `source` is the resource as the host read it, `None` when there's no such
/// resource.
pub async fn put_translation<S: HasTranslations>(
    ctx: &Ctx,
    ports: &S,
    resource: ResourceRef,
    language: &str,
    languages: StoreLanguages<'_>,
    source: Option<&Source>,
    fields: BTreeMap<String, FieldInput>,
) -> Result<Option<Translation>, CommandError> {
    write_translation(
        ctx,
        ports,
        resource,
        language,
        languages,
        source,
        |t, source| t.replace(source, fields.clone()),
    )
    .await
}

/// Changes only the fields of a resource's translation into `language` that `fields`
/// names: a value sets one (blank removes it), `None` removes it, and every field left
/// out stays. Creates the translation when there's none; one left with no field is
/// deleted (`None`). Otherwise as `put_translation`.
pub async fn patch_translation<S: HasTranslations>(
    ctx: &Ctx,
    ports: &S,
    resource: ResourceRef,
    language: &str,
    languages: StoreLanguages<'_>,
    source: Option<&Source>,
    fields: BTreeMap<String, Option<FieldInput>>,
) -> Result<Option<Translation>, CommandError> {
    write_translation(
        ctx,
        ports,
        resource,
        language,
        languages,
        source,
        |t, source| t.patch(source, fields.clone()),
    )
    .await
}

/// One write of a translation: `edit` applied to the stored one (or a new one), then
/// inserted, updated or deleted when it's left with no field. Starts over from the
/// stored one when another write got in first.
async fn write_translation<S: HasTranslations>(
    ctx: &Ctx,
    ports: &S,
    resource: ResourceRef,
    language: &str,
    languages: StoreLanguages<'_>,
    source: Option<&Source>,
    edit: impl Fn(&mut Translation, &Source) -> Result<bool, TranslationError>,
) -> Result<Option<Translation>, CommandError> {
    require_write(ctx, resource.kind)?;
    let language = Language::new(language)?;
    let source = source.ok_or(CommandError::ResourceNotFound(resource))?;
    if language == *languages.default {
        return Err(CommandError::DefaultLanguage(language));
    }
    if !languages.others.contains(&language) {
        return Err(CommandError::LanguageNotListed(language));
    }
    let repo = ports.translations();
    let mut attempt = 0;
    loop {
        attempt += 1;
        let stored = repo.get(resource, &language).await?;
        let is_new = stored.is_none();
        let mut translation =
            stored.unwrap_or_else(|| Translation::new(resource, language.clone()));
        if !edit(&mut translation, source)? {
            return Ok((!is_new).then_some(translation));
        }
        let written = if translation.is_empty() {
            translation.mark_deleted();
            repo.delete(&mut translation).await.map(|()| None)
        } else if is_new {
            repo.insert(&mut translation)
                .await
                .map(|()| Some(translation))
        } else {
            repo.update(&mut translation)
                .await
                .map(|()| Some(translation))
        };
        match written {
            // Someone wrote the same translation in between: the edit starts over from
            // theirs, so a replace replaces it and a patch keeps what it left out.
            Err(RepoError::StaleVersion | RepoError::Duplicate(keys::TRANSLATION))
                if attempt < ATTEMPTS => {}
            other => return Ok(other?),
        }
    }
}

/// Removes a resource's translation into `language`. The resource itself isn't asked
/// for: a translation may outlive it until the cleanup runs.
pub async fn delete_translation<S: HasTranslations>(
    ctx: &Ctx,
    ports: &S,
    resource: ResourceRef,
    language: &str,
) -> Result<(), CommandError> {
    require_write(ctx, resource.kind)?;
    let language = Language::new(language)?;
    let repo = ports.translations();
    let mut translation = repo
        .get(resource, &language)
        .await?
        .ok_or(CommandError::NotFound { resource, language })?;
    translation.mark_deleted();
    repo.delete(&mut translation).await?;
    Ok(())
}
