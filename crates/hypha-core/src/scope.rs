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
    /// External apps: their secrets, webhooks and deliveries (design note 0016).
    #[serde(rename = "apps:read")]
    AppsRead,
    #[serde(rename = "apps:write")]
    AppsWrite,
    /// Recording shipments and their tracking, without changing or cancelling orders
    /// (design note 0018, FU9).
    #[serde(rename = "fulfillments:read")]
    FulfillmentsRead,
    #[serde(rename = "fulfillments:write")]
    FulfillmentsWrite,
    /// Managing sales channels (design note 0020, CH10). Publishing products to them is
    /// `products:write`.
    #[serde(rename = "channels:read")]
    ChannelsRead,
    #[serde(rename = "channels:write")]
    ChannelsWrite,
    /// The payment methods a store offers (design note 0027, PA9). Recording money
    /// received on an order is `orders:write`; reading an order's payments, `orders:read`.
    #[serde(rename = "payments:read")]
    PaymentsRead,
    #[serde(rename = "payments:write")]
    PaymentsWrite,
    /// The emails a store sends (design note 0028, NT10): templates, branding, sender
    /// and staff recipients, and the record of what was sent. Resending one is
    /// `notifications:write`.
    #[serde(rename = "notifications:read")]
    NotificationsRead,
    #[serde(rename = "notifications:write")]
    NotificationsWrite,
    /// Custom field definitions in the merchant's `custom` namespace (design note 0033,
    /// §2). Values are governed by their owner's scopes (`products:write`, …).
    #[serde(rename = "custom_fields:read")]
    CustomFieldsRead,
    #[serde(rename = "custom_fields:write")]
    CustomFieldsWrite,
    /// Personal data (design note 0029, AP10, §4.2): without these, a caller that isn't
    /// a staff session (an API key, an app) gets customers, orders and carts with the
    /// field absent and listed in `redacted`. First and last names.
    #[serde(rename = "customer_data:name")]
    CustomerDataName,
    /// Email addresses.
    #[serde(rename = "customer_data:email")]
    CustomerDataEmail,
    /// Phone numbers.
    #[serde(rename = "customer_data:phone")]
    CustomerDataPhone,
    /// Street lines, postal code, city and province (country and company stay visible).
    #[serde(rename = "customer_data:address")]
    CustomerDataAddress,
    /// Personal custom fields on customers and orders (design note 0033, §5.2; 0032
    /// CD9): a birthday, a gift message.
    #[serde(rename = "customer_data:profile")]
    CustomerDataProfile,
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
            Scope::AppsRead => "apps:read",
            Scope::AppsWrite => "apps:write",
            Scope::FulfillmentsRead => "fulfillments:read",
            Scope::FulfillmentsWrite => "fulfillments:write",
            Scope::ChannelsRead => "channels:read",
            Scope::ChannelsWrite => "channels:write",
            Scope::PaymentsRead => "payments:read",
            Scope::PaymentsWrite => "payments:write",
            Scope::NotificationsRead => "notifications:read",
            Scope::NotificationsWrite => "notifications:write",
            Scope::CustomFieldsRead => "custom_fields:read",
            Scope::CustomFieldsWrite => "custom_fields:write",
            Scope::CustomerDataName => "customer_data:name",
            Scope::CustomerDataEmail => "customer_data:email",
            Scope::CustomerDataPhone => "customer_data:phone",
            Scope::CustomerDataAddress => "customer_data:address",
            Scope::CustomerDataProfile => "customer_data:profile",
        }
    }

    /// The scope named `name` (`as_str`'s inverse), `None` for anything else.
    pub fn parse(name: &str) -> Option<Scope> {
        Scope::ALL.into_iter().find(|s| s.as_str() == name)
    }

    /// Whether this scope unlocks personal data (`customer_data:*`): consent screens
    /// list these apart, with the app's reason for each.
    pub fn is_personal_data(self) -> bool {
        Scope::CUSTOMER_DATA.contains(&self)
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
            Scope::AppsRead => Some(Scope::AppsWrite),
            Scope::FulfillmentsRead => Some(Scope::FulfillmentsWrite),
            Scope::ChannelsRead => Some(Scope::ChannelsWrite),
            Scope::PaymentsRead => Some(Scope::PaymentsWrite),
            Scope::NotificationsRead => Some(Scope::NotificationsWrite),
            Scope::CustomFieldsRead => Some(Scope::CustomFieldsWrite),
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
            | Scope::ShippingWrite
            | Scope::AppsWrite
            | Scope::FulfillmentsWrite
            | Scope::ChannelsWrite
            | Scope::PaymentsWrite
            | Scope::NotificationsWrite
            | Scope::CustomFieldsWrite
            | Scope::CustomerDataName
            | Scope::CustomerDataEmail
            | Scope::CustomerDataPhone
            | Scope::CustomerDataAddress
            | Scope::CustomerDataProfile => None,
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

    /// The personal-data scopes (AP10), in `ALL` order.
    pub const CUSTOMER_DATA: [Scope; 5] = [
        Scope::CustomerDataName,
        Scope::CustomerDataEmail,
        Scope::CustomerDataPhone,
        Scope::CustomerDataAddress,
        Scope::CustomerDataProfile,
    ];

    pub const ALL: [Scope; 44] = [
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
        Scope::AppsRead,
        Scope::AppsWrite,
        Scope::FulfillmentsRead,
        Scope::FulfillmentsWrite,
        Scope::ChannelsRead,
        Scope::ChannelsWrite,
        Scope::PaymentsRead,
        Scope::PaymentsWrite,
        Scope::NotificationsRead,
        Scope::NotificationsWrite,
        Scope::CustomFieldsRead,
        Scope::CustomFieldsWrite,
        Scope::CustomerDataName,
        Scope::CustomerDataEmail,
        Scope::CustomerDataPhone,
        Scope::CustomerDataAddress,
        Scope::CustomerDataProfile,
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
    fn parse_is_as_str_reversed() {
        for scope in Scope::ALL {
            assert_eq!(Scope::parse(scope.as_str()), Some(scope));
        }
        assert_eq!(Scope::parse("customer_data:ssn"), None);
        assert!(Scope::CustomerDataEmail.is_personal_data());
        assert!(Scope::CustomerDataProfile.is_personal_data());
        assert!(!Scope::CustomersRead.is_personal_data());
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
