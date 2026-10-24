//! A reward's value and target as JSON, in the API's shape: what an order keeps of the
//! discounts it got (orders store it without reading it), and what reads it back.

use std::collections::BTreeMap;

use hypha_core::{BasisPoints, CategoryId, MarketId, Money, ProductId, VariantId};
use serde::{Deserialize, Serialize};

use crate::discount::{Allocation, ItemSet, Items, PerMarket, Target, Value};

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ItemsJson {
    All,
    Products { ids: Vec<ProductId> },
    Variants { ids: Vec<VariantId> },
    Categories { ids: Vec<CategoryId> },
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AllocationJson {
    Each,
    Across,
    Cheapest { units: u32 },
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ValueJson {
    Percent {
        basis_points: i32,
    },
    Amount {
        amounts: BTreeMap<MarketId, Money>,
        allocation: AllocationJson,
    },
    FixedPrice {
        amounts: BTreeMap<MarketId, Money>,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum TargetJson {
    Items {
        items: ItemsJson,
        exclude_on_sale: bool,
    },
    Subtotal,
    Shipping {
        max_amount: Option<BTreeMap<MarketId, Money>>,
    },
}

fn amounts(p: &PerMarket) -> BTreeMap<MarketId, Money> {
    p.iter().collect()
}

/// `value` as JSON: `{"type": "percent", "basis_points": 1000}`, `{"type": "amount",
/// "amounts": {"mkt_…": Money}, "allocation": {"type": "across"}}`, `{"type":
/// "fixed_price", "amounts": …}`.
pub fn value_json(value: &Value) -> serde_json::Value {
    let json = match value {
        Value::Percent(bp) => ValueJson::Percent {
            basis_points: bp.value(),
        },
        Value::Amount {
            amounts: a,
            allocation,
        } => ValueJson::Amount {
            amounts: amounts(a),
            allocation: match allocation {
                Allocation::Each => AllocationJson::Each,
                Allocation::Across => AllocationJson::Across,
                Allocation::Cheapest { units } => AllocationJson::Cheapest { units: *units },
            },
        },
        Value::FixedPrice(a) => ValueJson::FixedPrice {
            amounts: amounts(a),
        },
    };
    // Tagged enums of IDs, numbers and money: serializing can't fail.
    serde_json::to_value(json).unwrap_or(serde_json::Value::Null)
}

/// `target` as JSON: `{"type": "items", "items": {"type": "all"}, "exclude_on_sale":
/// false}`, `{"type": "subtotal"}`, `{"type": "shipping", "max_amount": null}`.
pub fn target_json(target: &Target) -> serde_json::Value {
    let json = match target {
        Target::Items(set) => TargetJson::Items {
            items: match &set.items {
                Items::All => ItemsJson::All,
                Items::Products(ids) => ItemsJson::Products { ids: ids.clone() },
                Items::Variants(ids) => ItemsJson::Variants { ids: ids.clone() },
                Items::Categories(ids) => ItemsJson::Categories { ids: ids.clone() },
            },
            exclude_on_sale: set.exclude_on_sale,
        },
        Target::Subtotal => TargetJson::Subtotal,
        Target::Shipping { max_amount } => TargetJson::Shipping {
            max_amount: max_amount.as_ref().map(amounts),
        },
    };
    serde_json::to_value(json).unwrap_or(serde_json::Value::Null)
}

/// `value_json` read back; `None` for JSON in another shape.
pub fn value_from_json(json: &serde_json::Value) -> Option<Value> {
    Some(match serde_json::from_value(json.clone()).ok()? {
        ValueJson::Percent { basis_points } => Value::Percent(BasisPoints::new(basis_points).ok()?),
        ValueJson::Amount {
            amounts,
            allocation,
        } => Value::Amount {
            amounts: PerMarket::new(amounts),
            allocation: match allocation {
                AllocationJson::Each => Allocation::Each,
                AllocationJson::Across => Allocation::Across,
                AllocationJson::Cheapest { units } => Allocation::Cheapest { units },
            },
        },
        ValueJson::FixedPrice { amounts } => Value::FixedPrice(PerMarket::new(amounts)),
    })
}

/// `target_json` read back; `None` for JSON in another shape.
pub fn target_from_json(json: &serde_json::Value) -> Option<Target> {
    Some(match serde_json::from_value(json.clone()).ok()? {
        TargetJson::Items {
            items,
            exclude_on_sale,
        } => Target::Items(ItemSet {
            items: match items {
                ItemsJson::All => Items::All,
                ItemsJson::Products { ids } => Items::Products(ids),
                ItemsJson::Variants { ids } => Items::Variants(ids),
                ItemsJson::Categories { ids } => Items::Categories(ids),
            },
            exclude_on_sale,
        }),
        TargetJson::Subtotal => Target::Subtotal,
        TargetJson::Shipping { max_amount } => Target::Shipping {
            max_amount: max_amount.map(PerMarket::new),
        },
    })
}

#[cfg(test)]
mod tests {
    use hypha_core::Currency;
    use uuid::Uuid;

    use super::*;

    #[test]
    fn a_value_reads_back_as_written() {
        let value = Value::Amount {
            amounts: PerMarket::of(
                MarketId::from_uuid(Uuid::from_u128(1)),
                Money::new(500, Currency::EUR).unwrap(),
            ),
            allocation: Allocation::Cheapest { units: 2 },
        };
        assert_eq!(value_from_json(&value_json(&value)), Some(value));
    }

    #[test]
    fn a_target_reads_back_as_written() {
        let target = Target::Items(ItemSet {
            items: Items::Products(vec![ProductId::from_uuid(Uuid::from_u128(1))]),
            exclude_on_sale: true,
        });
        assert_eq!(target_from_json(&target_json(&target)), Some(target));
    }

    #[test]
    fn the_json_names_its_type() {
        assert_eq!(
            value_json(&Value::Percent(BasisPoints::new(1000).unwrap())),
            serde_json::json!({"type": "percent", "basis_points": 1000})
        );
        assert_eq!(
            target_json(&Target::Subtotal),
            serde_json::json!({"type": "subtotal"})
        );
    }
}
