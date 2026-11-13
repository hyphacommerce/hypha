//! Translations: a resource's text in the store's other languages, read and written
//! apart from the resource. Admin reads of the resources themselves always answer the
//! source language.

use std::collections::BTreeMap;

use axum::{Json, extract::State, http::StatusCode};
use hypha_core::PageRequest;
use hypha_core::{Language, Stamped};
use hypha_translations::{
    FieldCounts, FieldInput, FieldRule, HasTranslations, InvalidWorkPosition, ResourceKind,
    ResourceRef, SourceDigest, StoreLanguages, Translation, TranslationKey, TranslationSourceRepo,
    TranslationState, WorkPosition, delete_translation, get_translations, list_translations,
    put_translation, require_read, require_write, source_digests, translation_progress,
    translation_worklist,
};
use serde::{Deserialize, Serialize};

use super::{
    page::{ListQuery, PageParams, PageResponse},
    stamp_times,
    stores::store_as_system,
};
use crate::{
    error::ApiError,
    extract::{Caller, JsonBody, PathId, Query},
    state::{AppState, SharedState, Storage},
    translations::{Loaded, TranslatedValue, source},
};

/// The resource a translation is of.
#[derive(Serialize, utoipa::ToSchema)]
pub struct TranslatedResourceResponse {
    /// `product`, `category`, `custom_field_definition`, `shipping_option`,
    /// `payment_method`, `discount`, `checkout_rule`, `asset`, `store` or `search_facet`.
    kind: &'static str,
    /// The resource's ID (`prod_…`); the store's for `store` and `search_facet`.
    id: String,
}

impl From<ResourceRef> for TranslatedResourceResponse {
    fn from(resource: ResourceRef) -> Self {
        Self {
            kind: resource.kind.as_str(),
            id: resource.id_string(),
        }
    }
}

/// One field's translation into one language.
#[derive(Serialize, utoipa::ToSchema)]
pub struct FieldTranslationResponse {
    value: String,
    /// The digest of the source text it translated.
    source_digest: String,
    /// Whether the source text changed since: `source_digest` isn't the field's
    /// `digest` now. Storefronts still get the translation.
    outdated: bool,
}

/// One translatable field of a resource: its source text and its translations.
#[derive(Serialize, utoipa::ToSchema)]
pub struct TranslatableFieldResponse {
    /// `title`, or a child's field by the child's ID: `options.<opt_…>.name`,
    /// `option_values.<optval_…>.label`, `choices.<key>.label`, `facets.<key>.label`,
    /// a custom field value's `custom_fields.<cfdef_…>.value` (a product's or a
    /// category's) or `variant_custom_fields.<var_…>.<cfdef_…>.value` (a variant's, in
    /// its product's).
    key: String,
    /// `text` (plain, trimmed) or `html` (sanitised like a description). A custom field
    /// value's follows its definition's type: `text` for `text` (one line) and
    /// `multiline_text`, `html` for `rich_text`.
    format: &'static str,
    /// The longest translation: characters for `text`, bytes of HTML for `html`. A
    /// custom field value's is its definition's `max_length` in characters, else its
    /// type's cap (255 characters for `text`, 16 KiB for the others).
    max_length: usize,
    /// The shortest translation, in characters (of the sanitised HTML, for `html`): a
    /// custom field value's definition's `min_length`. `null` when there is none, as
    /// for every field that isn't a custom field value.
    min_length: Option<usize>,
    /// The text in the store's default language; empty when the resource has none.
    source: String,
    /// SHA-256 of `source`, in hex: send it back as a field's `source_digest` to say
    /// which text was translated.
    digest: String,
    /// By language tag; a language without a translation of this field is absent.
    translations: BTreeMap<String, FieldTranslationResponse>,
    /// For a custom field value, which one: its definition and variant. `null` for
    /// every other field.
    custom_field: Option<TranslatedValue>,
}

/// One language's translation of a resource, as a whole.
#[derive(Serialize, utoipa::ToSchema)]
pub struct TranslationStateResponse {
    language: Language,
    /// How many of the resource's fields it translates.
    translated: usize,
    /// How many of those translate a source text that has changed since.
    outdated: usize,
    /// RFC 3339.
    created_at: String,
    updated_at: String,
}

/// A resource's translatable fields with every language's translation.
#[derive(Serialize, utoipa::ToSchema)]
pub struct TranslationsResponse {
    resource: TranslatedResourceResponse,
    /// The language `source` is in.
    default_language: Language,
    /// The store's other languages, published or not: what the resource can be
    /// translated into.
    languages: Vec<Language>,
    /// Every field the resource has now, in the order an editor shows them. A
    /// translated field whose option, value, choice or facet is gone isn't listed.
    fields: Vec<TranslatableFieldResponse>,
    /// The languages it has a translation in, by tag.
    translations: Vec<TranslationStateResponse>,
}

impl TranslationsResponse {
    fn new(
        resource: ResourceRef,
        languages: &Languages,
        loaded: &Loaded,
        translations: &[Translation],
    ) -> Result<Self, ApiError> {
        let fields: Vec<TranslatableFieldResponse> = loaded
            .source
            .fields
            .iter()
            .filter_map(|field| {
                let (spec, _) = resource.kind.field(&field.key)?;
                let (format, max_length, min_length) = match field.rule_or(spec.rule) {
                    FieldRule::Text { max_chars } => ("text", max_chars, None),
                    FieldRule::Html { max_bytes } => ("html", max_bytes, None),
                    FieldRule::Value(rule) => {
                        let format = match rule.format {
                            hypha_translations::ValueFormat::Html => "html",
                            _ => "text",
                        };
                        (format, rule.max_length(), rule.min_chars)
                    }
                };
                Some(TranslatableFieldResponse {
                    key: field.key.clone(),
                    format,
                    max_length,
                    min_length,
                    source: field.text.clone(),
                    digest: field.digest.clone(),
                    translations: translations
                        .iter()
                        .filter_map(|t| {
                            let translated = t.fields().get(&field.key)?;
                            Some((
                                t.language().to_string(),
                                FieldTranslationResponse {
                                    value: translated.value.clone(),
                                    source_digest: translated.source_digest.clone(),
                                    outdated: translated.source_digest != field.digest,
                                },
                            ))
                        })
                        .collect(),
                    custom_field: loaded.values.get(&field.key).cloned(),
                })
            })
            .collect();
        let translations = translations
            .iter()
            .map(|t| {
                let (created_at, updated_at) = stamp_times(t.stamps())?;
                let language = t.language().as_str();
                let of_language = || fields.iter().filter_map(|f| f.translations.get(language));
                Ok(TranslationStateResponse {
                    language: t.language().clone(),
                    translated: of_language().count(),
                    outdated: of_language().filter(|f| f.outdated).count(),
                    created_at,
                    updated_at,
                })
            })
            .collect::<Result<_, ApiError>>()?;
        Ok(Self {
            resource: resource.into(),
            default_language: languages.default.clone(),
            languages: languages.others.clone(),
            fields,
            translations,
        })
    }
}

/// The store's languages, read as the system: translating needs `translations:*`, not
/// `stores:read`.
struct Languages {
    default: Language,
    others: Vec<Language>,
}

async fn languages<S: Storage>(
    state: &AppState<S>,
    ports: &S::Ports<'_>,
) -> Result<Languages, ApiError> {
    let store = store_as_system(state, ports).await?;
    Ok(Languages {
        default: store.default_language().clone(),
        others: store
            .languages()
            .iter()
            .map(|l| l.language.clone())
            .collect(),
    })
}

/// The resource a URL's `{kind}` and `{id}` name: an unknown kind is `404
/// unknown_resource_kind`, an ID that isn't the kind's `400 invalid_path`.
fn resource(kind: &str, id: &str) -> Result<ResourceRef, ApiError> {
    let kind: ResourceKind = kind.parse()?;
    ResourceRef::parse(kind, id).map_err(|e| {
        ApiError::invalid_path(format!(
            "`{id}` isn't a {kind}'s ID ({}_…): {e}",
            kind.id_prefix()
        ))
    })
}

/// A resource's translatable fields: each one's source text, its digest and every
/// language's translation, `outdated` when the source changed since it was translated.
/// Beside `translations:read` it takes the resource's own read scope (`products:read`
/// for products, categories and search facets, `custom_fields:read`, `shipping:read`,
/// `payments:read`, `discounts:read`, `checkout_rules:read`, `media:read`; `stores:read`
/// and `notifications:read` for the store).
#[utoipa::path(
    get,
    path = "/admin/translations/{kind}/{id}",
    params(
        ("kind" = String, Path, description = "The kind of resource: `product`, `category`, `custom_field_definition`, `shipping_option`, `payment_method`, `discount`, `checkout_rule`, `asset`, `store`, `search_facet`.", example = "product"),
        ("id" = String, Path, description = "The resource's ID; the store's for `store` and `search_facet`.", example = "prod_00000000000000000000000000000000"),
    ),
    responses((status = 200, description = "OK", body = TranslationsResponse)),
    extensions(("x-hypha-scopes" = json!(["translations:read"]))),
)]
pub async fn fetch<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId((kind, id)): PathId<(String, String)>,
) -> Result<Json<TranslationsResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let resource = resource(&kind, &id)?;
    require_read(&ctx, resource.kind)?;
    Ok(Json(view(&state, &ctx, &ports, resource).await?))
}

/// The resource with its translations as they are now, for a caller already checked.
async fn view<S: Storage>(
    state: &AppState<S>,
    ctx: &hypha_core::Ctx,
    ports: &S::Ports<'_>,
    resource: ResourceRef,
) -> Result<TranslationsResponse, ApiError> {
    let loaded = source(state, ports, resource)
        .await?
        .ok_or(hypha_translations::CommandError::ResourceNotFound(resource))?;
    let translations = get_translations(ctx, ports, resource).await?;
    let languages = languages(state, ports).await?;
    TranslationsResponse::new(resource, &languages, &loaded, &translations)
}

/// One field of a translation being written.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct FieldTranslationRequest {
    /// The translation, by the rule of the field it replaces. Blank removes the field:
    /// the source shows again.
    value: String,
    /// The field's `digest` as the translator read it. Given, it must still be the
    /// field's digest (`source_changed` otherwise) and the translation is recorded as
    /// up to date. Left out, a new or changed value is recorded against the source as
    /// it is now, and an unchanged value stays as it was (outdated, if it was).
    source_digest: Option<String>,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PutTranslationRequest {
    /// Every translated field of this language, by key. A field left out is removed, so
    /// send the whole language; no fields at all deletes the translation.
    fields: BTreeMap<String, FieldTranslationRequest>,
}

/// Sets a resource's translation into one of the store's other languages, replacing the
/// fields it had. Answers the resource's translations as they are then. Beside
/// `translations:write` it takes the resource's own read scope.
#[utoipa::path(
    put,
    path = "/admin/translations/{kind}/{id}/{language}",
    params(
        ("kind" = String, Path, description = "The kind of resource.", example = "product"),
        ("id" = String, Path, description = "The resource's ID; the store's for `store` and `search_facet`.", example = "prod_00000000000000000000000000000000"),
        ("language" = String, Path, description = "A language on the store's list, published or not; never its default.", example = "de"),
    ),
    request_body = PutTranslationRequest,
    responses((status = 200, description = "OK", body = TranslationsResponse)),
    extensions(("x-hypha-scopes" = json!(["translations:write"]))),
)]
pub async fn put<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId((kind, id, language)): PathId<(String, String, String)>,
    body: JsonBody<PutTranslationRequest>,
) -> Result<Json<TranslationsResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let resource = resource(&kind, &id)?;
    require_write(&ctx, resource.kind)?;
    let req = body.parse()?;
    let fields = req
        .fields
        .into_iter()
        .map(|(key, field)| {
            (
                key,
                FieldInput {
                    value: field.value,
                    source_digest: field.source_digest,
                },
            )
        })
        .collect();
    let loaded = source(&state, &ports, resource).await?;
    let languages = languages(&state, &ports).await?;
    put_translation(
        &ctx,
        &ports,
        resource,
        &language,
        StoreLanguages {
            default: &languages.default,
            others: &languages.others,
        },
        loaded.as_ref().map(|l| &l.source),
        fields,
    )
    .await?;
    // The write found the resource, so the source is there.
    let loaded = loaded.unwrap_or_default();
    let translations = get_translations(&ctx, &ports, resource).await?;
    Ok(Json(TranslationsResponse::new(
        resource,
        &languages,
        &loaded,
        &translations,
    )?))
}

/// Removes a resource's translation into one language: its source text shows again.
#[utoipa::path(
    delete,
    path = "/admin/translations/{kind}/{id}/{language}",
    params(
        ("kind" = String, Path, description = "The kind of resource.", example = "product"),
        ("id" = String, Path, description = "The resource's ID; the store's for `store` and `search_facet`.", example = "prod_00000000000000000000000000000000"),
        ("language" = String, Path, description = "The translation's language.", example = "de"),
    ),
    responses((status = 204, description = "No content")),
    extensions(("x-hypha-scopes" = json!(["translations:write"]))),
)]
pub async fn remove<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId((kind, id, language)): PathId<(String, String, String)>,
) -> Result<StatusCode, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let resource = resource(&kind, &id)?;
    delete_translation(&ctx, &ports, resource, &language).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Where a resource stands in a language: `missing` while any field with source text
/// has no translation in it, else `outdated` while any translates a source text that has
/// changed since, else `complete`.
#[derive(Serialize, utoipa::ToSchema, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum TranslationStatus {
    Missing,
    Outdated,
    Complete,
}

impl From<TranslationState> for TranslationStatus {
    fn from(state: TranslationState) -> Self {
        match state {
            TranslationState::Missing => TranslationStatus::Missing,
            TranslationState::Outdated => TranslationStatus::Outdated,
            TranslationState::Complete => TranslationStatus::Complete,
        }
    }
}

/// A resource's translation into one language in a list: which resource, which
/// language, how it covers the resource's fields with source text, which fields it has.
#[derive(Serialize, utoipa::ToSchema)]
pub struct TranslationSummaryResponse {
    resource: TranslatedResourceResponse,
    language: Language,
    state: TranslationStatus,
    /// The resource's fields with source text: a field it leaves empty has nothing to
    /// translate and isn't counted. 0 until the resource was first read for the
    /// worklist (in the background, after an upgrade).
    total: u32,
    /// Of those, the ones without a translation in `language`.
    missing: u32,
    /// Of those translated, the ones whose source text changed since.
    outdated: u32,
    /// The keys it translates (a key whose option, value, choice or facet is gone
    /// included, until the translation is next written); empty when it has none yet.
    fields: Vec<String>,
    /// RFC 3339; `null` when the resource has no translation into `language` yet.
    created_at: Option<String>,
    updated_at: Option<String>,
}

impl TranslationSummaryResponse {
    fn new(
        resource: ResourceRef,
        language: Language,
        counts: FieldCounts,
        translation: Option<&Translation>,
    ) -> Result<Self, ApiError> {
        let (created_at, updated_at) = match translation {
            Some(t) => {
                let (created, updated) = stamp_times(t.stamps())?;
                (Some(created), Some(updated))
            }
            None => (None, None),
        };
        Ok(Self {
            resource: resource.into(),
            language,
            state: counts.state().into(),
            total: counts.total,
            missing: counts.missing,
            outdated: counts.outdated,
            fields: translation
                .map(|t| t.fields().keys().cloned().collect())
                .unwrap_or_default(),
            created_at,
            updated_at,
        })
    }
}

pub type TranslationPage = PageResponse<TranslationSummaryResponse>;

#[derive(Deserialize, utoipa::IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct TranslationFilters {
    /// Only resources of this kind (and its read scope is needed); without it, of every
    /// kind the caller may read.
    kind: Option<String>,
    /// Only translations into this language. Required with `state`.
    language: Option<String>,
    /// `missing`, `outdated` or `complete`: the worklist. Lists every resource with
    /// source text in that state in `language`, whether it has a translation yet or not,
    /// by kind in the catalog's order (`product`, `category`, `custom_field_definition`,
    /// `shipping_option`, `payment_method`, `discount`, `checkout_rule`, `asset`,
    /// `store`, `search_facet`), then by ID. Without it, the translations there are, by
    /// kind name, then resource, then language.
    state: Option<String>,
}

/// Where a list of translations continues: a translation's key, or a worklist's
/// resource.
enum ListAfter {
    Translation(TranslationKey),
    Work(WorkPosition),
}

impl std::fmt::Display for ListAfter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ListAfter::Translation(key) => key.fmt(f),
            ListAfter::Work(position) => position.fmt(f),
        }
    }
}

impl std::str::FromStr for ListAfter {
    type Err = InvalidWorkPosition;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.parse() {
            Ok(key) => Ok(ListAfter::Translation(key)),
            Err(_) => s.parse().map(ListAfter::Work),
        }
    }
}

/// The store's translations, or what's left to translate. Without `state`: the
/// translations there are, by kind name, then resource, then language, each with its
/// state (a resource not yet read for the worklist is read for it). With `state` (and
/// `language`: `400 invalid_query` without it, `422 translation_into_default_language`
/// or `422 language_not_listed` for one that isn't another on the store's list): the
/// worklist, every resource with source text whose translation into `language` is
/// `missing` (some field untranslated, or no translation at all), `outdated` (every
/// field translated, some from a source text that changed since) or `complete`, in the
/// catalog's order of kinds, then by ID. Counts are in fields, and only fields with
/// source text count.
#[utoipa::path(
    get,
    path = "/admin/translations",
    params(TranslationFilters, PageParams),
    responses((status = 200, description = "OK", body = TranslationPage)),
    extensions(("x-hypha-scopes" = json!(["translations:read"]))),
)]
pub async fn list<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    query: ListQuery<TranslationFilters>,
) -> Result<Json<TranslationPage>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let (page, filters, scope) = query.parse::<ListAfter>()?;
    let kind = filters
        .kind
        .as_deref()
        .map(str::parse::<ResourceKind>)
        .transpose()
        .map_err(|e| ApiError::invalid_query(format!("`kind`: {e}")))?;
    let language = filters
        .language
        .as_deref()
        .map(Language::new)
        .transpose()
        .map_err(|e| ApiError::invalid_query(format!("`language`: {e}")))?;
    let wanted = filters
        .state
        .as_deref()
        .map(str::parse::<TranslationState>)
        .transpose()
        .map_err(|e| ApiError::invalid_query(format!("`state`: {e}")))?;
    let limit = page.limit;
    if let Some(wanted) = wanted {
        let language = language.ok_or_else(|| {
            ApiError::invalid_query("`state` needs `language`: the language it's the state in")
        })?;
        // Anything else would be all missing: there's nothing to translate into it.
        let store_languages = languages(&state, &ports).await?;
        if language == store_languages.default {
            return Err(hypha_translations::CommandError::DefaultLanguage(language).into());
        }
        if !store_languages.others.contains(&language) {
            return Err(hypha_translations::CommandError::LanguageNotListed(language).into());
        }
        let after = match page.after {
            None => None,
            Some(ListAfter::Work(position)) => Some(position.0),
            Some(ListAfter::Translation(_)) => return Err(ApiError::invalid_cursor()),
        };
        let found = translation_worklist(
            &ctx,
            &ports,
            kind,
            language.clone(),
            Some(wanted),
            PageRequest { after, limit },
        )
        .await?;
        return Ok(Json(PageResponse::new(
            found,
            &scope,
            |item| ListAfter::Work(WorkPosition(item.resource)),
            |item| {
                TranslationSummaryResponse::new(
                    item.resource,
                    language.clone(),
                    item.counts,
                    item.translation.as_ref(),
                )
            },
        )?));
    }
    let after = match page.after {
        None => None,
        Some(ListAfter::Translation(key)) => Some(key),
        Some(ListAfter::Work(_)) => return Err(ApiError::invalid_cursor()),
    };
    let found =
        list_translations(&ctx, &ports, kind, language, PageRequest { after, limit }).await?;
    // Each translation's state, from its resource's digests: one more read for the page.
    let mut resources: Vec<ResourceRef> = found.items.iter().map(Translation::resource).collect();
    resources.dedup();
    let mut sources: BTreeMap<ResourceRef, Vec<SourceDigest>> = BTreeMap::new();
    for (resource, digest) in ports.translations().sources(&resources).await? {
        sources.entry(resource).or_default().push(digest);
    }
    // A resource without digests may not have been read for the worklist yet (after an
    // upgrade, or a read that failed): read it now, so its state isn't a guess.
    let unread: Vec<ResourceRef> = resources
        .into_iter()
        .filter(|r| !sources.contains_key(r))
        .collect();
    for resource in unread {
        if let Some(loaded) = source(&state, &ports, resource).await? {
            sources.insert(resource, source_digests(&loaded.source));
        }
    }
    Ok(Json(PageResponse::new(
        found,
        &scope,
        |t| ListAfter::Translation(TranslationKey::of(t)),
        |t| {
            let digests = sources.get(&t.resource()).map_or(&[][..], Vec::as_slice);
            TranslationSummaryResponse::new(
                t.resource(),
                t.language().clone(),
                FieldCounts::of(digests, Some(&t)),
                Some(&t),
            )
        },
    )?))
}

/// How far one language's translations cover the store's text, in fields with source
/// text: `translated` + `missing` = `total`, and `outdated` are among `translated`.
#[derive(Serialize, utoipa::ToSchema)]
pub struct LanguageProgressResponse {
    language: Language,
    total: u64,
    /// Translated, outdated or not.
    translated: u64,
    /// Translated from a source text that has changed since.
    outdated: u64,
    /// Without a translation.
    missing: u64,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct TranslationProgressResponse {
    /// The language the source text is in.
    default_language: Language,
    /// Each of the store's other languages, published or not, in the store's order.
    languages: Vec<LanguageProgressResponse>,
}

#[derive(Deserialize, utoipa::IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct ProgressQuery {
    /// Only this kind of resource (and its read scope is needed); without it, every kind
    /// the caller may read.
    kind: Option<String>,
}

/// How far each of the store's languages covers its text: every field with source text
/// of the kinds the caller may read, counted as translated (outdated among them) or
/// missing. What Settings → Languages shows; the resources behind the numbers are
/// `GET /admin/translations?language=&state=`.
#[utoipa::path(
    get,
    path = "/admin/translations/progress",
    params(ProgressQuery),
    responses((status = 200, description = "OK", body = TranslationProgressResponse)),
    extensions(("x-hypha-scopes" = json!(["translations:read"]))),
)]
pub async fn progress<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    query: Query<ProgressQuery>,
) -> Result<Json<TranslationProgressResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let query = query.parse()?;
    let kind = query
        .kind
        .as_deref()
        .map(str::parse::<ResourceKind>)
        .transpose()
        .map_err(|e| ApiError::invalid_query(format!("`kind`: {e}")))?;
    let languages = languages(&state, &ports).await?;
    let counted = translation_progress(&ctx, &ports, kind, &languages.others).await?;
    Ok(Json(TranslationProgressResponse {
        default_language: languages.default,
        languages: counted
            .into_iter()
            .map(|p| LanguageProgressResponse {
                missing: p.missing(),
                language: p.language,
                total: p.total,
                translated: p.translated,
                outdated: p.outdated,
            })
            .collect(),
    }))
}
