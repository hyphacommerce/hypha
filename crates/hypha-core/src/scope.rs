use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
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
    /// Staff users, roles and invites (design note 0008).
    #[serde(rename = "users:read")]
    UsersRead,
    #[serde(rename = "users:write")]
    UsersWrite,
    #[serde(rename = "api_keys:read")]
    ApiKeysRead,
    #[serde(rename = "api_keys:write")]
    ApiKeysWrite,
    /// The media library: assets and their files (design note 0013).
    #[serde(rename = "media:read")]
    MediaRead,
    #[serde(rename = "media:write")]
    MediaWrite,
    /// Shipping zones, options and providers (design note 0015).
    #[serde(rename = "shipping:read")]
    ShippingRead,
    #[serde(rename = "shipping:write")]
    ShippingWrite,
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
            Scope::UsersRead => "users:read",
            Scope::UsersWrite => "users:write",
            Scope::ApiKeysRead => "api_keys:read",
            Scope::ApiKeysWrite => "api_keys:write",
            Scope::MediaRead => "media:read",
            Scope::MediaWrite => "media:write",
            Scope::ShippingRead => "shipping:read",
            Scope::ShippingWrite => "shipping:write",
        }
    }

    /// The scope that also grants this one: a write scope grants its read scope, as in
    /// Shopify (design note 0008, A7). Roles and keys can then list `products:write`
    /// alone. `None` for write scopes and `checkout`.
    pub fn implied_by(self) -> Option<Scope> {
        match self {
            Scope::ProductsRead => Some(Scope::ProductsWrite),
            Scope::MarketsRead => Some(Scope::MarketsWrite),
            Scope::PricingRead => Some(Scope::PricingWrite),
            Scope::CartsRead => Some(Scope::CartsWrite),
            Scope::TaxesRead => Some(Scope::TaxesWrite),
            Scope::InventoryRead => Some(Scope::InventoryWrite),
            Scope::CustomersRead => Some(Scope::CustomersWrite),
            Scope::StoresRead => Some(Scope::StoresWrite),
            Scope::OrdersRead => Some(Scope::OrdersWrite),
            Scope::UsersRead => Some(Scope::UsersWrite),
            Scope::ApiKeysRead => Some(Scope::ApiKeysWrite),
            Scope::MediaRead => Some(Scope::MediaWrite),
            Scope::ShippingRead => Some(Scope::ShippingWrite),
            Scope::ProductsWrite
            | Scope::MarketsWrite
            | Scope::PricingWrite
            | Scope::CartsWrite
            | Scope::TaxesWrite
            | Scope::InventoryWrite
            | Scope::CustomersWrite
            | Scope::StoresWrite
            | Scope::OrdersWrite
            | Scope::Checkout
            | Scope::UsersWrite
            | Scope::ApiKeysWrite
            | Scope::MediaWrite
            | Scope::ShippingWrite => None,
        }
    }

    /// What a publishable API key grants: enough for a storefront to show products and
    /// run a cart through checkout, nothing that changes the catalog, customers or
    /// orders (design note 0008, section 4). `carts:write` implies `carts:read`.
    /// In `ALL` order, like every stored scope list.
    pub const STOREFRONT: [Scope; 5] = [
        Scope::ProductsRead,
        Scope::MarketsRead,
        Scope::CartsWrite,
        Scope::StoresRead,
        Scope::Checkout,
    ];

    pub const ALL: [Scope; 27] = [
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
        Scope::UsersRead,
        Scope::UsersWrite,
        Scope::ApiKeysRead,
        Scope::ApiKeysWrite,
        Scope::MediaRead,
        Scope::MediaWrite,
        Scope::ShippingRead,
        Scope::ShippingWrite,
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
    fn every_read_scope_is_implied_by_its_write_scope() {
        for scope in Scope::ALL {
            if let Some(read) = scope.as_str().strip_suffix(":read") {
                assert_eq!(
                    scope.implied_by().map(|s| s.to_string()),
                    Some(format!("{read}:write"))
                );
            } else {
                assert_eq!(scope.implied_by(), None, "{scope}");
            }
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
