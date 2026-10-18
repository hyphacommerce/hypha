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
    InMemoryOrderRepo, InMemoryOutbox, InMemoryPasswordResetRepo, InMemoryPaymentMethodRepo,
    InMemoryPaymentRepo, InMemoryPriceListRepo, InMemoryProductRepo, InMemoryRoleRepo,
    InMemorySearchQueue, InMemorySearchSettingsRepo, InMemorySessionRepo, InMemoryShippingZoneRepo,
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
    pub fulfillments: InMemoryFulfillmentRepo,
    // Payment methods and payments (design note 0027).
    pub payment_methods: InMemoryPaymentMethodRepo,
    pub payments: InMemoryPaymentRepo,
    pub assets: InMemoryAssetRepo,
    pub shipping_zones: InMemoryShippingZoneRepo,
    pub app_providers: InMemoryAppProviderRepo,
    pub apps: InMemoryAppRepo,
    /// OAuth's requests, codes and tokens (design note 0029), linked to `apps`.
    pub oauth: crate::InMemoryOAuth,
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
    // Email templates, settings and the record of messages (design note 0028); the
    // sender is passed apart.
    pub notification_templates: crate::InMemoryNotificationTemplateRepo,
    pub notification_settings: crate::InMemoryNotificationSettingsRepo,
    pub notifications: crate::InMemoryNotificationRepo,
    // Calls to apps' extension points (design note 0029).
    pub extension_calls: crate::InMemoryExtensionCallRepo,
    /// Where apps answer extension points, linked to `apps`.
    pub extension_endpoints: crate::InMemoryExtensionEndpointRepo,
    // The store itself (design note 0006, model A: its row is in its own database), so
    // the ports are its directory too, as a host's are.
    pub stores: crate::InMemoryStoreRepo,
}

impl Default for InMemoryPorts {
    fn default() -> Self {
        let outbox = InMemoryOutbox::default();
        let (apps, app_providers, oauth, extension_endpoints) = crate::apps::linked(&outbox);
        let (payment_methods, payments) = crate::payments::linked(&outbox);
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
            payment_methods,
            payments,
            assets: InMemoryAssetRepo::default(),
            shipping_zones: InMemoryShippingZoneRepo::sharing(&outbox),
            app_providers,
            apps,
            oauth,
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
            notification_templates: crate::InMemoryNotificationTemplateRepo::sharing(&outbox),
            notification_settings: crate::InMemoryNotificationSettingsRepo::sharing(&outbox),
            notifications: crate::InMemoryNotificationRepo::sharing(&outbox),
            extension_calls: crate::InMemoryExtensionCallRepo::default(),
            extension_endpoints,
            stores: crate::InMemoryStoreRepo::sharing(&outbox),
            outbox,
        }
    }
}

impl hypha_oauth::HasOAuthRequests for InMemoryPorts {
    type OAuthRequests = crate::InMemoryOAuth;
    fn oauth_requests(&self) -> &crate::InMemoryOAuth {
        &self.oauth
    }
}

impl hypha_oauth::HasOAuthCodes for InMemoryPorts {
    type OAuthCodes = crate::InMemoryOAuth;
    fn oauth_codes(&self) -> &crate::InMemoryOAuth {
        &self.oauth
    }
}

impl hypha_oauth::HasOAuthTokens for InMemoryPorts {
    type OAuthTokens = crate::InMemoryOAuth;
    fn oauth_tokens(&self) -> &crate::InMemoryOAuth {
        &self.oauth
    }
}

impl hypha_notifications::HasNotificationTemplates for InMemoryPorts {
    type NotificationTemplates = crate::InMemoryNotificationTemplateRepo;
    fn notification_templates(&self) -> &crate::InMemoryNotificationTemplateRepo {
        &self.notification_templates
    }
}

impl hypha_notifications::HasNotificationSettings for InMemoryPorts {
    type NotificationSettings = crate::InMemoryNotificationSettingsRepo;
    fn notification_settings(&self) -> &crate::InMemoryNotificationSettingsRepo {
        &self.notification_settings
    }
}

impl hypha_notifications::HasNotifications for InMemoryPorts {
    type Notifications = crate::InMemoryNotificationRepo;
    fn notifications(&self) -> &crate::InMemoryNotificationRepo {
        &self.notifications
    }
}

/// The store's directory through its ports, as a host's (`Storage::directory_in`).
impl hypha_stores::StoreRepo for InMemoryPorts {
    async fn get(&self, id: hypha_core::StoreId) -> Result<Option<hypha_stores::Store>, RepoError> {
        self.stores.get(id).await
    }

    async fn list_all(&self) -> Result<Vec<hypha_stores::Store>, RepoError> {
        self.stores.list_all().await
    }

    async fn insert(&self, store: &mut hypha_stores::Store) -> Result<(), RepoError> {
        self.stores.insert(store).await
    }

    async fn update(&self, store: &mut hypha_stores::Store) -> Result<(), RepoError> {
        self.stores.update(store).await
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

impl hypha_payments::HasPaymentMethods for InMemoryPorts {
    type PaymentMethods = InMemoryPaymentMethodRepo;
    fn payment_methods(&self) -> &InMemoryPaymentMethodRepo {
        &self.payment_methods
    }
}

impl hypha_payments::HasPayments for InMemoryPorts {
    type Payments = InMemoryPaymentRepo;
    fn payments(&self) -> &InMemoryPaymentRepo {
        &self.payments
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

impl hypha_extensions::HasExtensionCalls for InMemoryPorts {
    type ExtensionCalls = crate::InMemoryExtensionCallRepo;
    fn extension_calls(&self) -> &crate::InMemoryExtensionCallRepo {
        &self.extension_calls
    }
}

impl hypha_apps::HasExtensionEndpoints for InMemoryPorts {
    type ExtensionEndpoints = crate::InMemoryExtensionEndpointRepo;
    fn extension_endpoints(&self) -> &crate::InMemoryExtensionEndpointRepo {
        &self.extension_endpoints
    }
}

/// An app answers a point as `hypha_apps::find_endpoint` says (installed or made by
/// hand, enabled, granted the point's scope, with an endpoint for it): what hosts
/// answer from the same repos.
impl hypha_core::AppPoints for InMemoryPorts {
    async fn answers(
        &self,
        app: &hypha_core::AppKey,
        point: &str,
    ) -> Result<bool, hypha_core::RepoError> {
        let Some(k) = hypha_extensions::known_point(point) else {
            return Ok(false);
        };
        let point = hypha_apps::KnownPoint {
            name: k.name,
            version: k.version,
            scope: k.scope,
        };
        Ok(hypha_apps::find_endpoint(self, app, point).await?.is_some())
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
    /// The method `pay_by_hand` chooses.
    pub const BANK_TRANSFER: u128 = 0x0ba2_c7f2;

    /// Makes `cart` pay by bank transfer (design note 0027): a `manual` method offered
    /// everywhere, made on first use with instructions, chosen on the cart directly (no
    /// command, no checks). For tests of checkout that aren't about payments.
    pub fn pay_by_hand(&self, cart: hypha_core::CartId) -> hypha_core::PaymentMethodId {
        use hypha_cart::CartRepo;
        use hypha_payments::{NewPaymentMethod, PaymentMethod, PaymentMethodRepo, ProviderKey};
        let id = hypha_core::PaymentMethodId::from_uuid(uuid::Uuid::from_u128(Self::BANK_TRANSFER));
        if now_or_never(self.payment_methods.get(id))
            .unwrap_or_else(|e| panic!("{e}"))
            .is_none()
        {
            let mut method = PaymentMethod::new(NewPaymentMethod {
                id,
                provider: ProviderKey::manual(),
                name: "Bank transfer".to_string(),
                instructions: Some("IBAN HR12 3456 7890; quote your order number".to_string()),
                markets: vec![],
                channels: vec![],
                enabled: true,
                position: 0,
                settings: serde_json::json!({}),
            })
            .unwrap_or_else(|e| panic!("{e}"));
            now_or_never(self.payment_methods.insert(&mut method))
                .unwrap_or_else(|e| panic!("{e}"));
        }
        let mut stored = now_or_never(self.carts.get(cart))
            .unwrap_or_else(|e| panic!("{e}"))
            .unwrap_or_else(|| panic!("no cart {cart}"));
        stored
            .set_payment_method(Some(id))
            .unwrap_or_else(|e| panic!("{e}"));
        now_or_never(self.carts.update(&mut stored)).unwrap_or_else(|e| panic!("{e}"));
        id
    }

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
