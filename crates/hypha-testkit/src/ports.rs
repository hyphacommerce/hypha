use hypha_cart::HasCarts;
use hypha_core::HasIdempotency;
use hypha_customers::HasCustomers;
use hypha_inventory::{HasInventoryItems, HasLocations};
use hypha_markets::{HasCatalogs, HasMarkets, HasPriceLists};
use hypha_products::HasProducts;
use hypha_tax::{HasTaxClasses, HasTaxRegions};

use crate::{
    InMemoryCartRepo, InMemoryCatalogRepo, InMemoryCustomerRepo, InMemoryIdempotencyRepo,
    InMemoryInventoryItemRepo, InMemoryLocationRepo, InMemoryMarketRepo, InMemoryPriceListRepo,
    InMemoryProductRepo, InMemoryTaxClassRepo, InMemoryTaxRegionRepo,
};

/// Every repository of one store, in memory: the in-memory host's storage and the
/// tests' world. It has every accessor, so it is `hypha_cart::CartPorts`; the fields are
/// public so tests can set data up directly.
#[derive(Default)]
pub struct InMemoryPorts {
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
