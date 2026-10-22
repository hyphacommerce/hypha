//! The in-memory repos must pass the same contract as every storage adapter.

use hypha_core::Timestamp;
use hypha_testkit::{
    InMemoryAppRepo, InMemoryAssetRepo, InMemoryBlobStore, InMemoryCartRepo, InMemoryCatalogRepo,
    InMemoryCustomerRepo, InMemoryFulfillmentRepo, InMemoryInventoryItemRepo, InMemoryLocationRepo,
    InMemoryMarketRepo, InMemoryOrderRepo, InMemoryPorts, InMemoryPriceListRepo,
    InMemoryProductRepo, InMemorySearchIndex, InMemorySearchQueue, InMemorySearchSettingsRepo,
    InMemoryStoreRepo, InMemoryTaxClassRepo, InMemoryTaxRegionRepo, contract,
};
use pollster::block_on;

#[test]
fn agent_repo() {
    block_on(contract::agent_repo(async || InMemoryPorts::default()));
}

#[test]
fn proposal_repo() {
    block_on(contract::proposal_repo(async || InMemoryPorts::default()));
}

#[test]
fn agent_call_repo() {
    block_on(contract::agent_call_repo(async || InMemoryPorts::default()));
}

#[test]
fn product_repo() {
    block_on(contract::product_repo(async || {
        InMemoryProductRepo::default()
    }));
}

#[test]
fn product_list() {
    block_on(contract::product_list(async || InMemoryPorts::default()));
}

#[test]
fn market_repo() {
    block_on(contract::market_repo(async || {
        InMemoryMarketRepo::default()
    }));
}

#[test]
fn catalog_repo() {
    block_on(contract::catalog_repo(async || {
        InMemoryCatalogRepo::default()
    }));
}

#[test]
fn price_list_repo() {
    block_on(contract::price_list_repo(async || {
        InMemoryPriceListRepo::default()
    }));
}

#[test]
fn cart_repo() {
    block_on(contract::cart_repo(async || InMemoryCartRepo::default()));
}

#[test]
fn tax_class_repo() {
    block_on(contract::tax_class_repo(async || {
        InMemoryTaxClassRepo::default()
    }));
}

#[test]
fn tax_region_repo() {
    block_on(contract::tax_region_repo(async || {
        InMemoryTaxRegionRepo::default()
    }));
}

#[test]
fn location_repo() {
    block_on(contract::location_repo(async || {
        InMemoryLocationRepo::default()
    }));
}

#[test]
fn inventory_item_repo() {
    block_on(contract::inventory_item_repo(async || {
        InMemoryInventoryItemRepo::default()
    }));
}

#[test]
fn customer_repo() {
    block_on(contract::customer_repo(async || {
        InMemoryCustomerRepo::default()
    }));
}

#[test]
fn store_repo() {
    block_on(contract::store_repo(async || InMemoryStoreRepo::default()));
}

#[test]
fn outbox() {
    block_on(contract::outbox(async || InMemoryPorts::default()));
}

#[test]
fn batch() {
    block_on(contract::batch(async || InMemoryPorts::default()));
}

#[test]
fn preview_token_repo() {
    block_on(contract::preview_token_repo(async || {
        InMemoryPorts::default()
    }));
}

#[test]
fn idempotency_repo() {
    block_on(contract::idempotency_repo(async || {
        InMemoryPorts::default()
    }));
}

#[test]
fn extension_call_repo() {
    block_on(contract::extension_call_repo(async || {
        InMemoryPorts::default()
    }));
}

#[test]
fn extension_endpoint_repo() {
    block_on(contract::extension_endpoint_repo(async || {
        InMemoryPorts::default()
    }));
}

#[test]
fn fulfillment_repo() {
    block_on(contract::fulfillment_repo(async || {
        InMemoryFulfillmentRepo::default()
    }));
}

#[test]
fn order_repo() {
    block_on(contract::order_repo(async || InMemoryOrderRepo::default()));
}

#[test]
fn identity_repo() {
    block_on(contract::identity_repo(async || {
        hypha_testkit::InMemoryIdentityRepo::default()
    }));
}

#[test]
fn session_repo() {
    block_on(contract::session_repo(async || {
        hypha_testkit::InMemorySessionRepo::default()
    }));
}

#[test]
fn api_key_repo() {
    block_on(contract::api_key_repo(async || {
        hypha_testkit::InMemoryApiKeyRepo::default()
    }));
}

#[test]
fn password_reset_repo() {
    block_on(contract::password_reset_repo(async || {
        hypha_testkit::InMemoryPasswordResetRepo::default()
    }));
}

#[test]
fn user_repo() {
    block_on(contract::user_repo(async || {
        hypha_testkit::InMemoryUserRepo::default()
    }));
}

#[test]
fn role_repo() {
    block_on(contract::role_repo(async || {
        hypha_testkit::InMemoryRoleRepo::default()
    }));
}

#[test]
fn invite_repo() {
    block_on(contract::invite_repo(async || {
        hypha_testkit::InMemoryInviteRepo::default()
    }));
}

#[test]
fn staff_sessions() {
    block_on(contract::staff_sessions(async || InMemoryPorts::default()));
}

#[test]
fn staff_batch() {
    block_on(contract::staff_batch(async || InMemoryPorts::default()));
}

#[test]
fn asset_repo() {
    block_on(contract::asset_repo(async || InMemoryAssetRepo::default()));
}

#[test]
fn asset_usages() {
    block_on(contract::asset_usages(async || InMemoryPorts::default()));
}

#[test]
fn deletions() {
    block_on(contract::deletions(async || InMemoryPorts::default()));
}

#[test]
fn blob_store() {
    let store = InMemoryBlobStore::default();
    let now = Timestamp::from_unix_micros(1_767_225_600_000_000);
    block_on(contract::blob_store(
        &store,
        now,
        async |upload, body| store.upload(upload, body, now),
        async |url| store.download(url, now),
    ));
}

#[test]
fn shipping_zone_repo() {
    block_on(contract::shipping_zone_repo(async || {
        hypha_testkit::InMemoryShippingZoneRepo::default()
    }));
}

#[test]
fn app_provider_repo() {
    block_on(contract::app_provider_repo(async || {
        InMemoryPorts::default()
    }));
}

#[test]
fn app_repo() {
    block_on(contract::app_repo(async || InMemoryAppRepo::default()));
}

#[test]
fn spore_settings_repo() {
    block_on(contract::spore_settings_repo(async || {
        hypha_testkit::InMemorySporeSettingsRepo::default()
    }));
}

#[test]
fn oauth_repos() {
    block_on(contract::oauth_repos(async || InMemoryPorts::default()));
}

#[test]
fn app_deliveries() {
    block_on(contract::app_deliveries(async || InMemoryPorts::default()));
}

#[test]
fn search_index() {
    block_on(contract::search_index(async || {
        InMemorySearchIndex::default()
    }));
}

#[test]
fn search_queue() {
    block_on(contract::search_queue(async || {
        InMemorySearchQueue::default()
    }));
}

#[test]
fn search_settings_repo() {
    block_on(contract::search_settings_repo(async || {
        InMemorySearchSettingsRepo::default()
    }));
}

#[test]
fn channel_repo() {
    block_on(contract::channel_repo(async || InMemoryPorts::default()));
}

#[test]
fn category_repo() {
    block_on(contract::category_repo(async || InMemoryPorts::default()));
}

#[test]
fn customer_group_repo() {
    block_on(contract::customer_group_repo(async || {
        InMemoryPorts::default()
    }));
}

#[test]
fn payment_method_repo() {
    block_on(contract::payment_method_repo(async || {
        hypha_testkit::InMemoryPaymentMethodRepo::default()
    }));
}

#[test]
fn payment_method_delete() {
    block_on(contract::payment_method_delete(async || {
        InMemoryPorts::default()
    }));
}

#[test]
fn payment_repo() {
    block_on(contract::payment_repo(async || {
        hypha_testkit::InMemoryPaymentRepo::default()
    }));
}

#[test]
fn payment_batch() {
    block_on(contract::payment_batch(async || InMemoryPorts::default()));
}

#[test]
fn notification_template_repo() {
    block_on(contract::notification_template_repo(async || {
        hypha_testkit::InMemoryNotificationTemplateRepo::default()
    }));
}

#[test]
fn notification_settings_repo() {
    block_on(contract::notification_settings_repo(async || {
        hypha_testkit::InMemoryNotificationSettingsRepo::default()
    }));
}

#[test]
fn notification_repo() {
    block_on(contract::notification_repo(async || {
        hypha_testkit::InMemoryNotificationRepo::default()
    }));
}

#[test]
fn notification_settings_batch() {
    block_on(contract::notification_settings_batch(async || {
        InMemoryPorts::default()
    }));
}

#[test]
fn custom_field_definitions() {
    block_on(contract::custom_field_definitions(async || {
        InMemoryPorts::default()
    }));
}

#[test]
fn custom_field_values() {
    block_on(contract::custom_field_values(async || {
        InMemoryPorts::default()
    }));
}

#[test]
fn custom_field_filters() {
    block_on(contract::custom_field_filters(async || {
        InMemoryPorts::default()
    }));
}
