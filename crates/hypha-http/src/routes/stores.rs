use axum::{Json, extract::State};
use hypha_core::{Ctx, Currency, Email, Language, Scope, StoreId, Timezone};
use hypha_inventory::{HasStockMovements, StockMovementRepo};
use hypha_markets::{HasMarkets, MarketRepo};
use hypha_purchasing::{HasPurchaseOrders, PurchaseOrderRepo};
use hypha_stores::{
    LanguageInput, MarketLanguages, Store, StoreLanguage, UpdateStore, get_store, update_store,
};
use hypha_translations::{HasTranslations, TranslationRepo};
use serde::{Deserialize, Serialize};

use super::{
    address::{PostalAddressRequest, PostalAddressResponse},
    rfc3339,
};
use crate::{
    error::ApiError,
    extract::{Caller, JsonBody, Storefront},
    patch,
    state::{AppState, SharedState, Storage},
};

#[derive(Serialize, utoipa::ToSchema)]
pub struct StoreResponse {
    id: StoreId,
    name: String,
    /// Where shoppers and agents can write to the store.
    contact_email: Option<Email>,
    /// Plain text, at most 500 characters.
    description: Option<String>,
    /// The registered business name, for invoices and legal pages.
    legal_name: Option<String>,
    legal_address: Option<PostalAddressResponse>,
    /// Clients convert times with it; the API keeps every time in UTC.
    timezone: Option<Timezone>,
    /// The language the store's content (titles, descriptions, option labels) is written
    /// in.
    default_language: Language,
    /// The languages the content is translated into beside the default one, in the
    /// merchant's order.
    languages: Vec<StoreLanguageResponse>,
    /// The base currency every cost (purchase orders, stock value, an order line's cost)
    /// is kept in; `null` until it's chosen, and it must be before any cost is recorded.
    currency: Option<Currency>,
    /// How far the admin's setup guide got.
    setup: SetupProgressResponse,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct StoreLanguageResponse {
    language: Language,
    /// Whether shoppers are served it: an unpublished language can be translated, but no
    /// storefront read answers in it and no market offers it.
    published: bool,
}

impl From<&StoreLanguage> for StoreLanguageResponse {
    fn from(l: &StoreLanguage) -> Self {
        Self {
            language: l.language.clone(),
            published: l.published,
        }
    }
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct SetupProgressResponse {
    /// The step the merchant was last on, as the admin named it; `null` before they
    /// started.
    step: Option<String>,
    /// When they finished the guide or put it away, RFC 3339; `null` while it should
    /// still show.
    finished_at: Option<String>,
}

impl StoreResponse {
    fn new(s: &Store) -> Result<Self, ApiError> {
        let mut response = Self::from(s);
        response.setup.finished_at = s.setup().finished_at.map(rfc3339).transpose()?;
        Ok(response)
    }
}

impl From<&Store> for StoreResponse {
    fn from(s: &Store) -> Self {
        Self {
            id: s.id(),
            name: s.name().to_string(),
            contact_email: s.contact_email().cloned(),
            description: s.description().map(str::to_string),
            legal_name: s.legal_name().map(str::to_string),
            legal_address: s.legal_address().map(PostalAddressResponse::from),
            timezone: s.timezone(),
            default_language: s.default_language().clone(),
            languages: s.languages().iter().map(Into::into).collect(),
            currency: s.currency(),
            setup: SetupProgressResponse {
                step: s
                    .setup()
                    .step
                    .as_ref()
                    .map(|step| step.as_str().to_string()),
                finished_at: None,
            },
        }
    }
}

/// The store this request is served by.
#[utoipa::path(
    get,
    path = "/admin/store",
    responses((status = 200, description = "OK", body = StoreResponse)),
    extensions(("x-hypha-scopes" = json!(["stores:read"]))),
)]
pub async fn fetch<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
) -> Result<Json<StoreResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let store = read_store(&state, &ctx, ports).await?;
    Ok(Json(StoreResponse::new(&store)?))
}

/// A JSON Merge Patch of the store: absent fields stay as they are.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateStoreRequest {
    /// 1 to 100 characters, trimmed.
    #[serde(default, deserialize_with = "patch::non_null")]
    // Optional, but `null` is refused (`patch::non_null`): not nullable in the spec.
    #[schema(value_type = String, required = false)]
    name: Option<String>,
    /// `null` clears it.
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<String>, required = false)]
    contact_email: Option<Option<String>>,
    /// Plain text, at most 500 characters, trimmed; `null` or blank clears it.
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<String>, required = false)]
    description: Option<Option<String>>,
    /// At most 255 characters, trimmed; `null` or blank clears it.
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<String>, required = false)]
    legal_name: Option<Option<String>>,
    /// Replaced whole; `null` clears it.
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<PostalAddressRequest>, required = false)]
    legal_address: Option<Option<PostalAddressRequest>>,
    /// An IANA name such as `Europe/Belgrade`; `null` clears it.
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<String>, required = false)]
    timezone: Option<Option<String>>,
    /// A BCP 47 tag such as `en` or `sr-Latn`. Not one of `languages` (`409
    /// default_language_listed`, unless the same request takes it off that list), and
    /// markets that name the old one must still be offered it (`409 language_in_use`).
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = String, required = false)]
    default_language: Option<String>,
    /// The other languages, replacing the list: at most 20, each once, never the default
    /// one. A language markets offer can't be removed or unpublished (`409
    /// language_in_use`, `details.markets: [{id, languages}]` for callers with
    /// `markets:read`).
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = Vec<StoreLanguageRequest>, required = false)]
    languages: Option<Vec<StoreLanguageRequest>>,
    /// An ISO 4217 code: the base currency costs are kept in. `null` clears it.
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<String>, required = false)]
    currency: Option<Option<String>>,
    /// The setup guide step the merchant is on: 1 to 40 lowercase letters, digits, `-` or
    /// `_`, named by the admin. `null` clears it.
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<String>, required = false)]
    setup_step: Option<Option<String>>,
    /// `true` marks the guide done (the first time sets `finished_at`); `false` shows it
    /// again from its step.
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = bool, required = false)]
    setup_finished: Option<bool>,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct StoreLanguageRequest {
    /// A BCP 47 tag such as `de` or `sr-Latn`.
    language: String,
    /// Whether shoppers are served it; `false` when absent.
    #[serde(default)]
    published: bool,
}

/// Changes the store's details.
#[utoipa::path(
    patch,
    path = "/admin/store",
    request_body = UpdateStoreRequest,
    responses((status = 200, description = "OK", body = StoreResponse)),
    extensions(("x-hypha-scopes" = json!(["stores:write"]))),
)]
pub async fn update<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    body: JsonBody<UpdateStoreRequest>,
) -> Result<Json<StoreResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    // Recorded costs fix the currency, and so does a purchase order priced against it
    // (ordered, or a draft with a rate to it). Checked before the write, not in its
    // transaction: a cost recorded at the same instant as a currency change can slip
    // through.
    let currency_locked = match req.currency {
        Some(_) => {
            ports.stock_movements().cost_recorded().await?
                || ports.purchase_orders().store_currency_named().await?
        }
        None => false,
    };
    // What markets offer holds the store to offering it, checked like the currency:
    // before the write, so a market changed at the same instant can slip through.
    let market_languages = if req.default_language.is_some() || req.languages.is_some() {
        let markets = ports.markets().list_all().await?;
        markets
            .iter()
            .map(|m| MarketLanguages {
                market: m.id(),
                languages: m.languages().to_vec(),
            })
            .collect()
    } else {
        Vec::new()
    };
    // Every translation is one from the default language, which fixes it; checked like
    // the currency, before the write.
    let has_translations = match req.default_language {
        Some(_) => ports.translations().any().await?,
        None => false,
    };
    let input = UpdateStore {
        name: req.name,
        contact_email: req.contact_email,
        description: req.description,
        legal_name: req.legal_name,
        legal_address: req.legal_address.map(|a| a.map(Into::into)),
        timezone: req.timezone,
        default_language: req.default_language,
        languages: req.languages.map(|languages| {
            languages
                .into_iter()
                .map(|l| LanguageInput {
                    language: l.language,
                    published: l.published,
                })
                .collect()
        }),
        market_languages,
        currency: req.currency,
        currency_locked,
        has_translations,
        setup_step: req.setup_step,
        setup_finished: req.setup_finished,
    };
    // Through the request's ports when they hold the directory (`read_store`).
    let updated = match state.storage.directory_in(&ports) {
        Some(directory) => update_store(&ctx, directory, &state.clock, state.store, input).await,
        None => {
            drop(ports);
            let directory = state.storage.directory().await?;
            update_store(&ctx, &directory, &state.clock, state.store, input).await
        }
    };
    let store = updated.map_err(|e| {
        // Which markets are in the way is theirs to read: `stores:write` alone gets the
        // code.
        let names_markets = matches!(e, hypha_stores::CommandError::LanguageInUse(_));
        let error = ApiError::from(e);
        if names_markets && !ctx.has_scope(Scope::MarketsRead) {
            return error.with_details(None);
        }
        error
    })?;
    Ok(Json(StoreResponse::new(&store)?))
}

/// What a storefront sees of the store: what a header, footer or contact page shows. Legal
/// details stay in the admin until a storefront needs them.
#[derive(Serialize, utoipa::ToSchema)]
pub struct StorefrontStoreResponse {
    name: String,
    /// Plain text, at most 500 characters.
    description: Option<String>,
    contact_email: Option<Email>,
    /// The language the store's content is written in.
    default_language: Language,
}

/// The store's public details.
#[utoipa::path(
    get,
    path = "/storefront/store",
    responses((status = 200, description = "OK", body = StorefrontStoreResponse)),
    extensions(("x-hypha-scopes" = json!(["stores:read"]))),
)]
pub async fn fetch_storefront<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller<Storefront>,
) -> Result<Json<StorefrontStoreResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let store = read_store(&state, &ctx, ports).await?;
    Ok(Json(StorefrontStoreResponse {
        name: store.name().to_string(),
        description: store.description().map(str::to_string),
        contact_email: store.contact_email().cloned(),
        default_language: store.default_language().clone(),
    }))
}

/// The store's details: through the request's `ports` when the storage keeps the
/// directory there (`Storage::directory_in`), else through the directory, once `ports`
/// (and their connection) are let go.
pub(crate) async fn read_store<S: Storage>(
    state: &AppState<S>,
    ctx: &Ctx,
    ports: S::Ports<'_>,
) -> Result<Store, ApiError> {
    if let Some(directory) = state.storage.directory_in(&ports) {
        return Ok(get_store(ctx, directory, state.store).await?);
    }
    drop(ports);
    let directory = state.storage.directory().await?;
    Ok(get_store(ctx, &directory, state.store).await?)
}

/// The store's base currency, which every cost is kept in, for the routes that record
/// one. Read as the system: recording a cost needs `purchasing:write`, not `stores:read`.
pub(crate) async fn store_currency<S: Storage>(
    state: &AppState<S>,
    ports: &S::Ports<'_>,
) -> Result<Option<Currency>, ApiError> {
    Ok(store_as_system(state, ports).await?.currency())
}

/// The languages shoppers may be served, the store's default first, for the routes that
/// hold a market's languages to them. Read as the system: a market's languages need
/// `markets:write` or a storefront's `markets:read`, not `stores:read`.
pub(crate) async fn offered_languages<S: Storage>(
    state: &AppState<S>,
    ports: &S::Ports<'_>,
) -> Result<Vec<Language>, ApiError> {
    Ok(store_as_system(state, ports).await?.offered_languages())
}

pub(crate) async fn store_as_system<S: Storage>(
    state: &AppState<S>,
    ports: &S::Ports<'_>,
) -> Result<Store, ApiError> {
    let ctx = Ctx::system();
    Ok(match state.storage.directory_in(ports) {
        Some(directory) => get_store(&ctx, directory, state.store).await?,
        None => {
            let directory = state.storage.directory().await?;
            get_store(&ctx, &directory, state.store).await?
        }
    })
}
