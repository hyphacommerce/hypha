use hypha_auth::{HasApiKeys, HasIdentities, HasPasswordResets, HasSessions};
use hypha_cart::HasCarts;
use hypha_core::{ActingAs, Actor, HasIdempotency, HasOutbox};
use hypha_customers::HasCustomers;
use hypha_inventory::{HasInventoryItems, HasLocations};
use hypha_markets::{HasCatalogs, HasMarkets, HasPriceLists};
use hypha_media::{HasAssetUsages, HasAssets};
use hypha_orders::HasOrders;
use hypha_products::HasProducts;
use hypha_shipping::{HasAppProviders, HasShippingZones};
use hypha_tax::{HasTaxClasses, HasTaxRegions};
use hypha_users::{HasInvites, HasRoles, HasUsers};

use crate::{
    InMemoryApiKeyRepo, InMemoryAppProviderRepo, InMemoryAssetRepo, InMemoryCartRepo,
    InMemoryCatalogRepo, InMemoryCustomerRepo, InMemoryIdempotencyRepo, InMemoryIdentityRepo,
    InMemoryInventoryItemRepo, InMemoryInviteRepo, InMemoryLocationRepo, InMemoryMarketRepo,
    InMemoryOrderRepo, InMemoryOutbox, InMemoryPasswordResetRepo, InMemoryPriceListRepo,
    InMemoryProductRepo, InMemoryRoleRepo, InMemorySessionRepo, InMemoryShippingZoneRepo,
    InMemoryTaxClassRepo, InMemoryTaxRegionRepo, InMemoryUserRepo,
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
    pub assets: InMemoryAssetRepo,
    pub shipping_zones: InMemoryShippingZoneRepo,
    pub app_providers: InMemoryAppProviderRepo,
    // Staff and keys live in the store's database (design note 0006, T4).
    pub identities: InMemoryIdentityRepo,
    pub sessions: InMemorySessionRepo,
    pub api_keys: InMemoryApiKeyRepo,
    pub password_resets: InMemoryPasswordResetRepo,
    pub users: InMemoryUserRepo,
    pub roles: InMemoryRoleRepo,
    pub invites: InMemoryInviteRepo,
}

impl Default for InMemoryPorts {
    fn default() -> Self {
        let outbox = InMemoryOutbox::default();
        Self {
            carts: InMemoryCartRepo::sharing(&outbox),
            products: InMemoryProductRepo::sharing(&outbox),
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
            assets: InMemoryAssetRepo::default(),
            shipping_zones: InMemoryShippingZoneRepo::sharing(&outbox),
            app_providers: InMemoryAppProviderRepo::sharing(&outbox),
            identities: InMemoryIdentityRepo::default(),
            sessions: InMemorySessionRepo::default(),
            api_keys: InMemoryApiKeyRepo::default(),
            password_resets: InMemoryPasswordResetRepo::default(),
            users: InMemoryUserRepo::default(),
            roles: InMemoryRoleRepo::default(),
            invites: InMemoryInviteRepo::default(),
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

impl HasInvites for InMemoryPorts {
    type Invites = InMemoryInviteRepo;
    fn invites(&self) -> &InMemoryInviteRepo {
        &self.invites
    }
}
