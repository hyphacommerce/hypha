//! What custom fields belong to (design note 0033, §6.1). The leaf can't see products
//! or customers: routes and workflows check an owner exists through its own repository
//! before passing it here.

use std::{fmt, str::FromStr};

use hypha_core::{CategoryId, CustomerId, OrderId, ProductId, Scope, VariantId};
use uuid::Uuid;

/// The kinds of record that hold custom fields in v1 (Q3). More later (markets,
/// locations, carts) with no model change: a value row names only its owner's ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OwnerType {
    Product,
    Variant,
    /// Product categories (design note 0022).
    Category,
    Customer,
    Order,
}

impl OwnerType {
    pub const ALL: [OwnerType; 5] = [
        OwnerType::Product,
        OwnerType::Variant,
        OwnerType::Category,
        OwnerType::Customer,
        OwnerType::Order,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            OwnerType::Product => "product",
            OwnerType::Variant => "variant",
            OwnerType::Category => "category",
            OwnerType::Customer => "customer",
            OwnerType::Order => "order",
        }
    }

    /// The scope that reads an owner of this type, and so its `custom.*` fields (§5.1).
    /// Categories are part of the catalog, like their commands.
    pub fn read_scope(self) -> Scope {
        match self {
            OwnerType::Product | OwnerType::Variant | OwnerType::Category => Scope::ProductsRead,
            OwnerType::Customer => Scope::CustomersRead,
            OwnerType::Order => Scope::OrdersRead,
        }
    }

    /// The scope that writes an owner of this type, and its fields.
    pub fn write_scope(self) -> Scope {
        match self {
            OwnerType::Product | OwnerType::Variant | OwnerType::Category => Scope::ProductsWrite,
            OwnerType::Customer => Scope::CustomersWrite,
            OwnerType::Order => Scope::OrdersWrite,
        }
    }

    /// `id` as this type's prefixed ID (`prod_…`, `var_…`, `pcat_…`, `cus_…`, `ord_…`).
    pub fn format_id(self, id: Uuid) -> String {
        match self {
            OwnerType::Product => ProductId::from_uuid(id).to_string(),
            OwnerType::Variant => VariantId::from_uuid(id).to_string(),
            OwnerType::Category => CategoryId::from_uuid(id).to_string(),
            OwnerType::Customer => CustomerId::from_uuid(id).to_string(),
            OwnerType::Order => OrderId::from_uuid(id).to_string(),
        }
    }

    /// Whether its definitions may be `personal` (§2: only customers and orders).
    pub fn allows_personal(self) -> bool {
        matches!(self, OwnerType::Customer | OwnerType::Order)
    }

    /// The event its writes record (§6.4). Variants report on their product's.
    pub fn event_type(self) -> &'static str {
        match self {
            OwnerType::Product | OwnerType::Variant => "product.custom_fields_updated",
            OwnerType::Category => "category.custom_fields_updated",
            OwnerType::Customer => "customer.custom_fields_updated",
            OwnerType::Order => "order.custom_fields_updated",
        }
    }
}

impl fmt::Display for OwnerType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for OwnerType {
    /// The text that isn't an owner type.
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        OwnerType::ALL
            .into_iter()
            .find(|t| t.as_str() == s)
            .ok_or_else(|| s.to_string())
    }
}

/// One owner: its type and ID, and for a variant its product (the event is the
/// product's, §6.4). Built from the typed IDs, so a value can't be given to the wrong
/// kind of record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Owner {
    kind: OwnerType,
    id: Uuid,
    /// The variant's product; `None` for every other kind.
    product: Option<ProductId>,
}

impl Owner {
    pub fn product(id: ProductId) -> Self {
        Self {
            kind: OwnerType::Product,
            id: id.as_uuid(),
            product: None,
        }
    }

    pub fn variant(product: ProductId, id: VariantId) -> Self {
        Self {
            kind: OwnerType::Variant,
            id: id.as_uuid(),
            product: Some(product),
        }
    }

    pub fn category(id: CategoryId) -> Self {
        Self {
            kind: OwnerType::Category,
            id: id.as_uuid(),
            product: None,
        }
    }

    pub fn customer(id: CustomerId) -> Self {
        Self {
            kind: OwnerType::Customer,
            id: id.as_uuid(),
            product: None,
        }
    }

    pub fn order(id: OrderId) -> Self {
        Self {
            kind: OwnerType::Order,
            id: id.as_uuid(),
            product: None,
        }
    }

    /// An owner as a value row names it: its type (the definition's), ID and, for a
    /// variant, its product (`StoredValue::owner_parent`).
    pub fn from_parts(kind: OwnerType, id: Uuid, parent: Option<Uuid>) -> Self {
        Self {
            kind,
            id,
            product: match kind {
                OwnerType::Variant => parent.map(ProductId::from_uuid),
                _ => None,
            },
        }
    }

    pub fn kind(&self) -> OwnerType {
        self.kind
    }

    /// The owner's UUID: what a value row stores.
    pub fn id(&self) -> Uuid {
        self.id
    }

    /// A variant's product.
    pub fn product_of_variant(&self) -> Option<ProductId> {
        self.product
    }

    /// The owner as its prefixed ID, `prod_…`: what errors and responses show.
    pub fn prefixed(&self) -> String {
        self.kind.format_id(self.id)
    }
}

impl fmt::Display for Owner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.prefixed())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_types_parse_their_own_strings_only() {
        for t in OwnerType::ALL {
            assert_eq!(t.as_str().parse(), Ok(t));
        }
        assert_eq!("cart".parse::<OwnerType>(), Err("cart".to_string()));
    }

    #[test]
    fn only_customers_and_orders_may_hold_personal_fields() {
        let personal: Vec<OwnerType> = OwnerType::ALL
            .into_iter()
            .filter(|t| t.allows_personal())
            .collect();
        assert_eq!(personal, [OwnerType::Customer, OwnerType::Order]);
    }

    #[test]
    fn a_variant_reports_on_its_product() {
        let product = ProductId::from_uuid(Uuid::from_u128(1));
        let owner = Owner::variant(product, VariantId::from_uuid(Uuid::from_u128(2)));
        assert_eq!(owner.product_of_variant(), Some(product));
        assert_eq!(owner.kind().event_type(), OwnerType::Product.event_type());
        assert!(owner.prefixed().starts_with("var_"));
    }
}
