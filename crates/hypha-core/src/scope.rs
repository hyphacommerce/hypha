use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Scope {
    #[serde(rename = "products:read")]
    ProductsRead,
    #[serde(rename = "products:write")]
    ProductsWrite,
    #[serde(rename = "markets:read")]
    MarketsRead,
    #[serde(rename = "markets:write")]
    MarketsWrite,
    #[serde(rename = "pricing:read")]
    PricingRead,
    #[serde(rename = "pricing:write")]
    PricingWrite,
    #[serde(rename = "carts:read")]
    CartsRead,
    #[serde(rename = "carts:write")]
    CartsWrite,
    #[serde(rename = "taxes:read")]
    TaxesRead,
    #[serde(rename = "taxes:write")]
    TaxesWrite,
    #[serde(rename = "inventory:read")]
    InventoryRead,
    #[serde(rename = "inventory:write")]
    InventoryWrite,
    #[serde(rename = "customers:read")]
    CustomersRead,
    #[serde(rename = "customers:write")]
    CustomersWrite,
    #[serde(rename = "stores:read")]
    StoresRead,
    #[serde(rename = "stores:write")]
    StoresWrite,
    #[serde(rename = "orders:read")]
    OrdersRead,
    /// Changing orders (marking them paid; later edits and refunds).
    #[serde(rename = "orders:write")]
    OrdersWrite,
    /// Turning a cart into an order: what a storefront needs, without the scopes of
    /// everything checkout writes underneath (design note 0007).
    #[serde(rename = "checkout")]
    Checkout,
}

impl Scope {
    /// The same string used in JSON, e.g. "products:write".
    pub fn as_str(&self) -> &'static str {
        match self {
            Scope::ProductsRead => "products:read",
            Scope::ProductsWrite => "products:write",
            Scope::MarketsRead => "markets:read",
            Scope::MarketsWrite => "markets:write",
            Scope::PricingRead => "pricing:read",
            Scope::PricingWrite => "pricing:write",
            Scope::CartsRead => "carts:read",
            Scope::CartsWrite => "carts:write",
            Scope::TaxesRead => "taxes:read",
            Scope::TaxesWrite => "taxes:write",
            Scope::InventoryRead => "inventory:read",
            Scope::InventoryWrite => "inventory:write",
            Scope::CustomersRead => "customers:read",
            Scope::CustomersWrite => "customers:write",
            Scope::StoresRead => "stores:read",
            Scope::StoresWrite => "stores:write",
            Scope::OrdersRead => "orders:read",
            Scope::OrdersWrite => "orders:write",
            Scope::Checkout => "checkout",
        }
    }

    pub const ALL: [Scope; 19] = [
        Scope::ProductsRead,
        Scope::ProductsWrite,
        Scope::MarketsRead,
        Scope::MarketsWrite,
        Scope::PricingRead,
        Scope::PricingWrite,
        Scope::CartsRead,
        Scope::CartsWrite,
        Scope::TaxesRead,
        Scope::TaxesWrite,
        Scope::InventoryRead,
        Scope::InventoryWrite,
        Scope::CustomersRead,
        Scope::CustomersWrite,
        Scope::StoresRead,
        Scope::StoresWrite,
        Scope::OrdersRead,
        Scope::OrdersWrite,
        Scope::Checkout,
    ];
}

impl fmt::Display for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn display_matches_json_for_every_scope() {
        for scope in Scope::ALL {
            assert_eq!(
                serde_json::to_string(&scope).unwrap(),
                format!("\"{scope}\"")
            );
        }
    }

    #[test]
    fn uses_colon_names_in_json() {
        assert_eq!(
            serde_json::to_string(&Scope::ProductsWrite).unwrap(),
            "\"products:write\""
        );
        assert_eq!(
            serde_json::from_str::<Scope>("\"markets:read\"").unwrap(),
            Scope::MarketsRead
        );
    }
}
