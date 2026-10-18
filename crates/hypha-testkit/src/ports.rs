use hypha_auth::{
    HasApiKeys, HasIdentities, HasPasswordResets, HasSessions, Principal, SessionRepo, TokenHash,
};
use hypha_cart::HasCarts;
use hypha_core::{ActingAs, Actor, HasIdempotency, HasOutbox, RepoError};
use hypha_customers::HasCustomers;
use hypha_fulfillment::HasFulfillments;
use hypha_inventory::{HasInventoryItems, HasLocations};
use hypha_markets::{HasCatalogs, HasMarkets, HasPriceLists};
use hypha_media::{HasAssetUsages, HasAssets};
use hypha_orders::HasOrders;
use hypha_products::HasProducts;
use hypha_shipping::{HasAppProviders, HasShippingZones};
use hypha_tax::{HasTaxClasses, HasTaxRegions};
use hypha_users::{
    HasInvites, HasRoles, HasStaffSessions, HasUsers, RoleRepo, StaffSessionRecord,
    StaffSessionRepo, UserRepo,
};

use crate::{
    InMemoryApiKeyRepo, InMemoryAppProviderRepo, InMemoryAppRepo, InMemoryAssetRepo,
    InMemoryCartRepo, InMemoryCatalogRepo, InMemoryCategoryRepo, InMemoryChannelRepo,
    InMemoryCustomerRepo, InMemoryFulfillmentRepo, InMemoryIdempotencyRepo, InMemoryIdentityRepo,
    InMemoryInventoryItemRepo, InMemoryInviteRepo, InMemoryLocationRepo, InMemoryMarketRepo,
    InMemoryOrderRepo, InMemoryOutbox, InMemoryPasswordResetRepo, InMemoryPriceListRepo,
    InMemoryProductRepo, InMemoryRoleRepo, InMemorySearchQueue, InMemorySearchSettingsRepo,
    InMemorySessionRepo, InMemoryShippingZoneRepo, InMemoryTaxClassRepo, InMemoryTaxRegionRepo,
    InMemoryUserRepo,
};

/// Every repository of one store, in memory: the tests' world. It has every accessor, so it is `hypha_cart::CartPorts`; the fields are
/// public so tests can set data up directly. One outbox is shared by every repo that
/// writes events and by batches, so `outbox.events()` is everything the store recorded.
pub struct InMemoryPorts {
    pub outbox: InMemoryOutbox,
    pub carts: InMemoryCartRepo,
    pub products: InMemoryProductRepo,
    pub markets: InMemoryMarketRepo,
    pub catalogs: InMemoryCatalogRepo,
    pub price_lists: InMemoryPriceListRepo,
    pub tax_classes: InMemoryTaxClassRepo,
    pub tax_regions: InMemoryTaxRegionRepo,
    pub inventory_items: InMemoryInventoryItemRepo,
    pub locations: InMemoryLocationRepo,
    pub customers: InMemoryCustomerRepo,
    pub idempotency: InMemoryIdempotencyRepo,
    pub orders: InMemoryOrderRepo,
    pub fulfillments: InMemoryFulfillmentRepo,
    pub assets: InMemoryAssetRepo,
    pub shipping_zones: InMemoryShippingZoneRepo,
    pub app_providers: InMemoryAppProviderRepo,
    pub apps: InMemoryAppRepo,
    // Staff and keys live in the store's database (design note 0006, T4).
    pub identities: InMemoryIdentityRepo,
    pub sessions: InMemorySessionRepo,
    pub api_keys: InMemoryApiKeyRepo,
    pub password_resets: InMemoryPasswordResetRepo,
    pub users: InMemoryUserRepo,
    pub roles: InMemoryRoleRepo,
    pub invites: InMemoryInviteRepo,
    // Sales channels and their publications (design note 0020).
    pub channels: InMemoryChannelRepo,
    // Categories and their membership (design note 0022).
    pub categories: InMemoryCategoryRepo,
    // Search's queue and settings (design note 0019); the engine is passed apart.
    pub search_queue: InMemorySearchQueue,
    pub search_settings: InMemorySearchSettingsRepo,
}

impl Default for InMemoryPorts {
    fn default() -> Self {
        let outbox = InMemoryOutbox::default();
        let (apps, app_providers) = crate::apps::linked(&outbox);
        let channels = InMemoryChannelRepo::sharing(&outbox);
        let categories = InMemoryCategoryRepo::sharing(&outbox);
        Self {
            carts: InMemoryCartRepo::sharing(&outbox),
            products: InMemoryProductRepo::sharing(&outbox).joining(&categories, &channels),
            markets: InMemoryMarketRepo::sharing(&outbox),
            catalogs: InMemoryCatalogRepo::sharing(&outbox),
            price_lists: InMemoryPriceListRepo::sharing(&outbox),
            tax_classes: InMemoryTaxClassRepo::sharing(&outbox),
            tax_regions: InMemoryTaxRegionRepo::sharing(&outbox),
            inventory_items: InMemoryInventoryItemRepo::sharing(&outbox),
            locations: InMemoryLocationRepo::sharing(&outbox),
            customers: InMemoryCustomerRepo::sharing(&outbox),
            idempotency: InMemoryIdempotencyRepo::default(),
            orders: InMemoryOrderRepo::sharing(&outbox),
            fulfillments: InMemoryFulfillmentRepo::sharing(&outbox),
            assets: InMemoryAssetRepo::default(),
            shipping_zones: InMemoryShippingZoneRepo::sharing(&outbox),
            app_providers,
            apps,
            identities: InMemoryIdentityRepo::default(),
            sessions: InMemorySessionRepo::default(),
            api_keys: InMemoryApiKeyRepo::default(),
            password_resets: InMemoryPasswordResetRepo::default(),
            users: InMemoryUserRepo::default(),
            roles: InMemoryRoleRepo::default(),
            invites: InMemoryInviteRepo::default(),
            channels,
            categories,
            search_queue: InMemorySearchQueue::default(),
            search_settings: InMemorySearchSettingsRepo::sharing(&outbox),
            outbox,
        }
    }
}

impl ActingAs for InMemoryPorts {
    fn act_as(&self, actor: Actor) {
        self.outbox.act_as(actor);
    }
}

impl HasOutbox for InMemoryPorts {
    type Outbox = InMemoryOutbox;
    fn outbox(&self) -> &InMemoryOutbox {
        &self.outbox
    }
}

impl HasCarts for InMemoryPorts {
    type Carts = InMemoryCartRepo;
    fn carts(&self) -> &InMemoryCartRepo {
        &self.carts
    }
}

impl HasProducts for InMemoryPorts {
    type Products = InMemoryProductRepo;
    fn products(&self) -> &InMemoryProductRepo {
        &self.products
    }
}

impl HasMarkets for InMemoryPorts {
    type Markets = InMemoryMarketRepo;
    fn markets(&self) -> &InMemoryMarketRepo {
        &self.markets
    }
}

impl HasCatalogs for InMemoryPorts {
    type Catalogs = InMemoryCatalogRepo;
    fn catalogs(&self) -> &InMemoryCatalogRepo {
        &self.catalogs
    }
}

impl HasPriceLists for InMemoryPorts {
    type PriceLists = InMemoryPriceListRepo;
    fn price_lists(&self) -> &InMemoryPriceListRepo {
        &self.price_lists
    }
}

impl HasTaxClasses for InMemoryPorts {
    type TaxClasses = InMemoryTaxClassRepo;
    fn tax_classes(&self) -> &InMemoryTaxClassRepo {
        &self.tax_classes
    }
}

impl HasTaxRegions for InMemoryPorts {
    type TaxRegions = InMemoryTaxRegionRepo;
    fn tax_regions(&self) -> &InMemoryTaxRegionRepo {
        &self.tax_regions
    }
}

impl HasInventoryItems for InMemoryPorts {
    type InventoryItems = InMemoryInventoryItemRepo;
    fn inventory_items(&self) -> &InMemoryInventoryItemRepo {
        &self.inventory_items
    }
}

impl HasLocations for InMemoryPorts {
    type Locations = InMemoryLocationRepo;
    fn locations(&self) -> &InMemoryLocationRepo {
        &self.locations
    }
}

impl HasCustomers for InMemoryPorts {
    type Customers = InMemoryCustomerRepo;
    fn customers(&self) -> &InMemoryCustomerRepo {
        &self.customers
    }
}

impl HasIdempotency for InMemoryPorts {
    type Idempotency = InMemoryIdempotencyRepo;
    fn idempotency(&self) -> &InMemoryIdempotencyRepo {
        &self.idempotency
    }
}

impl HasAssets for InMemoryPorts {
    type Assets = InMemoryAssetRepo;
    fn assets(&self) -> &InMemoryAssetRepo {
        &self.assets
    }
}

/// Usages are kept with the assets they point at (`InMemoryAssetRepo`).
impl HasAssetUsages for InMemoryPorts {
    type AssetUsages = InMemoryAssetRepo;
    fn asset_usages(&self) -> &InMemoryAssetRepo {
        &self.assets
    }
}

impl HasShippingZones for InMemoryPorts {
    type ShippingZones = InMemoryShippingZoneRepo;
    fn shipping_zones(&self) -> &InMemoryShippingZoneRepo {
        &self.shipping_zones
    }
}

impl HasAppProviders for InMemoryPorts {
    type AppProviders = InMemoryAppProviderRepo;
    fn app_providers(&self) -> &InMemoryAppProviderRepo {
        &self.app_providers
    }
}

impl HasFulfillments for InMemoryPorts {
    type Fulfillments = InMemoryFulfillmentRepo;
    fn fulfillments(&self) -> &InMemoryFulfillmentRepo {
        &self.fulfillments
    }
}

impl HasOrders for InMemoryPorts {
    type Orders = InMemoryOrderRepo;
    fn orders(&self) -> &InMemoryOrderRepo {
        &self.orders
    }
}

impl HasIdentities for InMemoryPorts {
    type Identities = InMemoryIdentityRepo;
    fn identities(&self) -> &InMemoryIdentityRepo {
        &self.identities
    }
}

impl HasSessions for InMemoryPorts {
    type Sessions = InMemorySessionRepo;
    fn sessions(&self) -> &InMemorySessionRepo {
        &self.sessions
    }
}

impl HasApiKeys for InMemoryPorts {
    type ApiKeys = InMemoryApiKeyRepo;
    fn api_keys(&self) -> &InMemoryApiKeyRepo {
        &self.api_keys
    }
}

impl HasPasswordResets for InMemoryPorts {
    type PasswordResets = InMemoryPasswordResetRepo;
    fn password_resets(&self) -> &InMemoryPasswordResetRepo {
        &self.password_resets
    }
}

impl HasUsers for InMemoryPorts {
    type Users = InMemoryUserRepo;
    fn users(&self) -> &InMemoryUserRepo {
        &self.users
    }
}

impl HasRoles for InMemoryPorts {
    type Roles = InMemoryRoleRepo;
    fn roles(&self) -> &InMemoryRoleRepo {
        &self.roles
    }
}

impl HasStaffSessions for InMemoryPorts {
    type StaffSessions = Self;
    fn staff_sessions(&self) -> &Self {
        self
    }
}

/// Three lookups in memory; one query in SQL adapters.
impl StaffSessionRepo for InMemoryPorts {
    async fn staff_session_by_token(
        &self,
        hash: TokenHash,
    ) -> Result<Option<StaffSessionRecord>, RepoError> {
        let Some(session) = self.sessions.get_by_token(hash).await? else {
            return Ok(None);
        };
        let Principal::Staff(user_id) = session.principal();
        let user = self.users.get(user_id).await?;
        let role = match &user {
            Some(user) => self.roles.get(user.role()).await?,
            None => None,
        };
        Ok(Some(StaffSessionRecord {
            session,
            user,
            role,
        }))
    }
}

impl HasInvites for InMemoryPorts {
    type Invites = InMemoryInviteRepo;
    fn invites(&self) -> &InMemoryInviteRepo {
        &self.invites
    }
}

impl hypha_apps::HasApps for InMemoryPorts {
    type Apps = InMemoryAppRepo;
    fn apps(&self) -> &InMemoryAppRepo {
        &self.apps
    }
}

impl hypha_search_index::HasSearchQueue for InMemoryPorts {
    type SearchQueue = InMemorySearchQueue;
    fn search_queue(&self) -> &InMemorySearchQueue {
        &self.search_queue
    }
}

impl hypha_search_index::HasSearchSettings for InMemoryPorts {
    type SearchSettings = InMemorySearchSettingsRepo;
    fn search_settings(&self) -> &InMemorySearchSettingsRepo {
        &self.search_settings
    }
}

impl hypha_categories::HasCategories for InMemoryPorts {
    type Categories = InMemoryCategoryRepo;
    fn categories(&self) -> &InMemoryCategoryRepo {
        &self.categories
    }
}

impl hypha_channels::HasChannels for InMemoryPorts {
    type Channels = InMemoryChannelRepo;
    fn channels(&self) -> &InMemoryChannelRepo {
        &self.channels
    }
}

impl hypha_auth::ChannelDirectory for InMemoryPorts {
    async fn channel_exists(
        &self,
        channel: hypha_core::ChannelId,
    ) -> Result<bool, hypha_core::RepoError> {
        Ok(hypha_channels::ChannelRepo::get(&self.channels, channel)
            .await?
            .is_some())
    }
}

impl InMemoryPorts {
    /// The channel `storefront` sells through.
    pub const ONLINE_STORE: u128 = 0x0c4a_77e1;

    /// A storefront's caller (design note 0020): in an "Online Store" channel, made on
    /// first use, with every product that exists now published there and every scope.
    /// For tests of what storefronts do; unpublish through `channels` to test that.
    pub fn storefront(&self) -> hypha_core::Ctx {
        use hypha_channels::{Channel, ChannelKind, ChannelRepo};
        let id = hypha_core::ChannelId::from_uuid(uuid::Uuid::from_u128(Self::ONLINE_STORE));
        if !self.channels.exists(id) {
            let mut channel = Channel::new(id, "Online Store", ChannelKind::OnlineStore)
                .unwrap_or_else(|e| panic!("{e}"));
            now_or_never(self.channels.insert(&mut channel)).unwrap_or_else(|e| panic!("{e}"));
        }
        let products: Vec<hypha_core::ProductId> = self
            .products
            .products
            .all()
            .iter()
            .map(|p| p.id())
            .collect();
        now_or_never(self.channels.publish(id, &products)).unwrap_or_else(|e| panic!("{e}"));
        hypha_core::Ctx::in_channel(
            hypha_core::Actor::System,
            hypha_core::Scope::ALL.to_vec(),
            id,
        )
    }
}

/// Runs an in-memory repo's future, which never waits on anything, to completion.
fn now_or_never<F: std::future::Future>(future: F) -> F::Output {
    use std::task::{Context, Poll, Waker};
    let mut future = std::pin::pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    match future.as_mut().poll(&mut cx) {
        Poll::Ready(output) => output,
        Poll::Pending => unreachable!("in-memory repos are ready at once"),
    }
}
