//! Translations: a resource's text in the store's other languages, read and written
//! apart from the resource. Admin reads of the resources themselves always answer the
//! source language.

use std::collections::BTreeMap;

use axum::{Json, extract::State, http::StatusCode};
use hypha_core::{Language, Stamped};
use hypha_translations::{
    FieldInput, FieldRule, ResourceKind, ResourceRef, StoreLanguages, Translation, TranslationKey,
    delete_translation, get_translations, list_translations, put_translation, require_read,
    require_write,
};
use serde::{Deserialize, Serialize};

use super::{
    page::{ListQuery, PageParams, PageResponse},
    stamp_times,
    stores::store_as_system,
};
use crate::{
    error::ApiError,
    extract::{Caller, JsonBody, PathId},
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
    /// type's cap (255 characters for `text`, 16 KiB for the others), and its
    /// `min_length` holds too.
    max_length: usize,
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
                let (format, max_length) = match field.rule_or(spec.rule) {
                    FieldRule::Text { max_chars } => ("text", max_chars),
                    FieldRule::Html { max_bytes } => ("html", max_bytes),
                    FieldRule::Value(rule) => match rule.format {
                        hypha_translations::ValueFormat::Html => ("html", rule.max_length()),
                        _ => ("text", rule.max_length()),
                    },
                };
                Some(TranslatableFieldResponse {
                    key: field.key.clone(),
                    format,
                    max_length,
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

/// A translation in a list: which resource, which language, which fields.
#[derive(Serialize, utoipa::ToSchema)]
pub struct TranslationSummaryResponse {
    resource: TranslatedResourceResponse,
    language: Language,
    /// The keys it translates. Read the resource's translations for the text.
    fields: Vec<String>,
    /// RFC 3339.
    created_at: String,
    updated_at: String,
}

pub type TranslationPage = PageResponse<TranslationSummaryResponse>;

#[derive(Deserialize, utoipa::IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct TranslationFilters {
    /// Only translations of this kind of resource (and its read scope is needed);
    /// without it, of every kind the caller may read.
    kind: Option<String>,
    /// Only translations into this language.
    language: Option<String>,
}

/// The store's translations, by kind, then resource, then language: what has been
/// translated. What's missing or outdated is each resource's to say (`GET
/// /admin/translations/{kind}/{id}`).
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
    let (page, filters, scope) = query.parse::<TranslationKey>()?;
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
    let found = list_translations(&ctx, &ports, kind, language, page).await?;
    Ok(Json(PageResponse::new(
        found,
        &scope,
        TranslationKey::of,
        |t| {
            let (created_at, updated_at) = stamp_times(t.stamps())?;
            Ok(TranslationSummaryResponse {
                resource: t.resource().into(),
                language: t.language().clone(),
                fields: t.fields().keys().cloned().collect(),
                created_at,
                updated_at,
            })
        },
    )?))
}
