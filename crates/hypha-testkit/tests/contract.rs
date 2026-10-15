//! The in-memory repos must pass the same contract as every storage adapter.

use hypha_core::Timestamp;
use hypha_testkit::{
    InMemoryAppRepo, InMemoryAssetRepo, InMemoryBlobStore, InMemoryCartRepo, InMemoryCatalogRepo,
    InMemoryCustomerRepo, InMemoryInventoryItemRepo, InMemoryLocationRepo, InMemoryMarketRepo,
    InMemoryOrderRepo, InMemoryPorts, InMemoryPriceListRepo, InMemoryProductRepo,
    InMemoryStoreRepo, InMemoryTaxClassRepo, InMemoryTaxRegionRepo, contract,
};
use pollster::block_on;

#[test]
fn product_repo() {
    block_on(contract::product_repo(async || {
        InMemoryProductRepo::default()
    }));
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
fn idempotency_repo() {
    block_on(contract::idempotency_repo(async || {
        InMemoryPorts::default()
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
fn app_deliveries() {
    block_on(contract::app_deliveries(async || InMemoryPorts::default()));
}
