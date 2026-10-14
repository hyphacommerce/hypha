//! The in-memory repos must pass the same contract as every storage adapter.

use hypha_testkit::{
    InMemoryCartRepo, InMemoryCatalogRepo, InMemoryCustomerRepo, InMemoryInventoryItemRepo,
    InMemoryLocationRepo, InMemoryMarketRepo, InMemoryPorts, InMemoryPriceListRepo,
    InMemoryProductRepo, InMemoryStoreRepo, InMemoryTaxClassRepo, InMemoryTaxRegionRepo, contract,
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
fn batch() {
    block_on(contract::batch(async || InMemoryPorts::default()));
}
