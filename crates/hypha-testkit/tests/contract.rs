//! The in-memory repos must pass the same contract as every storage adapter.

use hypha_testkit::{
    InMemoryCartRepo, InMemoryCatalogRepo, InMemoryMarketRepo, InMemoryPriceListRepo,
    InMemoryProductRepo, contract,
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
