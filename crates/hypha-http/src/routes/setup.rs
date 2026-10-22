//! The setup checklist: whether each first step is done, worked out from the data on every
//! call, never stored. One call for what the admin's home page asked of six routes plus two
//! per sampled product.

use axum::{Json, extract::State};
use hypha_auth::{ApiKeyKind, HasApiKeys, list_api_keys};
use hypha_channels::channels_of_products;
use hypha_core::{PageRequest, ProductId, Scope, VariantId};
use hypha_inventory::{HasInventoryItems, HasLocations, get_variants_inventory, list_locations};
use hypha_markets::list_markets;
use hypha_merchandising::list_product_views;
use hypha_products::{
    HasProducts, ListProducts, ProductFilterInput, ProductSort, ProductStatus, get_products,
};
use hypha_shipping::{HasShippingZones, list_zones};
use hypha_stores::get_store;
use serde::Serialize;

use crate::{
    error::ApiError,
    extract::Caller,
    state::{SharedState, Storage},
};

/// How many active products to look at for stock, as the admin's page did.
const STOCK_SAMPLE: usize = 5;

/// Each first step, done or not. A step the caller's scopes can't read counts as not
/// done, so the checklist answers whoever asks.
#[derive(Serialize, utoipa::ToSchema)]
pub struct SetupChecklistResponse {
    /// The store has a contact email shoppers can write to (`stores:read`).
    store: bool,
    /// A market exists (`markets:read`).
    market: bool,
    /// A shipping zone has an option (`shipping:read`).
    shipping: bool,
    /// A product is active (`products:read`).
    product: bool,
    /// A location exists, and one of the first active products can be sold: its first
    /// variant's stock isn't tracked, or some is available (`products:read`,
    /// `inventory:read`).
    stock: bool,
    /// An active product is published to a channel (`products:read`).
    publish: bool,
    /// A publishable key that isn't revoked exists (`api_keys:read`).
    key: bool,
    /// A payment method is enabled (`payments:read`): without one, checkout refuses carts
    /// with something to pay.
    payment: bool,
}

/// The setup checklist, in one call.
#[utoipa::path(
    get,
    path = "/admin/setup/checklist",
    responses((status = 200, description = "OK", body = SetupChecklistResponse)),
    extensions(("x-hypha-scopes" = json!([]))),
)]
pub async fn checklist<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
) -> Result<Json<SetupChecklistResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;

    let store = if ctx.has_scope(Scope::StoresRead) {
        let store = match state.storage.directory_in(&ports) {
            Some(directory) => get_store(&ctx, directory, state.store).await?,
            None => {
                let directory = state.storage.directory().await?;
                get_store(&ctx, &directory, state.store).await?
            }
        };
        store.contact_email().is_some()
    } else {
        false
    };

    let market = ctx.has_scope(Scope::MarketsRead) && !list_markets(&ctx, &ports).await?.is_empty();

    let shipping = ctx.has_scope(Scope::ShippingRead)
        && list_zones(&ctx, ports.shipping_zones())
            .await?
            .iter()
            .any(|zone| !zone.options().is_empty());

    let active: Vec<ProductId> = if ctx.has_scope(Scope::ProductsRead) {
        let input = ListProducts {
            filter: ProductFilterInput {
                statuses: vec![ProductStatus::Active],
                ..ProductFilterInput::default()
            },
            sort: ProductSort::default(),
            fields: Vec::new(),
        };
        list_product_views(&ctx, &ports, input, PageRequest::default())
            .await?
            .items
            .iter()
            .map(|view| view.summary.id)
            .collect()
    } else {
        Vec::new()
    };

    let publish = !active.is_empty()
        && channels_of_products(&ctx, &ports, &active)
            .await?
            .values()
            .any(|channels| !channels.is_empty());

    let stock = !active.is_empty()
        && ctx.has_scope(Scope::InventoryRead)
        && !list_locations(&ctx, ports.locations()).await?.is_empty()
        && {
            let sample = &active[..active.len().min(STOCK_SAMPLE)];
            let firsts: Vec<VariantId> = get_products(&ctx, ports.products(), sample)
                .await?
                .iter()
                .filter_map(|p| p.variants().first().map(|v| v.id()))
                .collect();
            let items = get_variants_inventory(&ctx, ports.inventory_items(), &firsts).await?;
            // A first variant without an item isn't tracked, so it can be sold.
            firsts.iter().any(|variant| {
                match items.iter().find(|item| item.variant() == *variant) {
                    None => true,
                    Some(item) => {
                        !item.tracked()
                            || item
                                .levels()
                                .iter()
                                .any(|level| item.available(level.location) > 0)
                    }
                }
            })
        };

    let key = ctx.has_scope(Scope::ApiKeysRead)
        && list_api_keys(&ctx, ports.api_keys(), PageRequest::default())
            .await?
            .items
            .iter()
            .any(|k| k.kind() == ApiKeyKind::Publishable && k.revoked_at().is_none());

    let payment = ctx.has_scope(Scope::PaymentsRead)
        && hypha_payments::list_payment_methods(&ctx, &ports)
            .await?
            .iter()
            .any(|m| m.enabled());

    Ok(Json(SetupChecklistResponse {
        store,
        market,
        shipping,
        product: !active.is_empty(),
        stock,
        publish,
        key,
        payment,
    }))
}
