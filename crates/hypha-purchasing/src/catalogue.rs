//! What a supplier sells: one row per (supplier, variant), with the supplier's own code
//! and price. Rows of their own rather than children of the supplier, which may list
//! thousands.

use hypha_core::{Currency, SupplierId, UnitCost, VariantId, optional_text};

use crate::{PurchasingError, supplier::check_lead_time};

/// A catalogue row as given: checked by `SupplierVariant::new`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SupplierVariantDraft {
    /// The supplier's code for it, printed on purchase orders.
    pub supplier_sku: Option<String>,
    /// What the supplier charges per unit, in the supplier's currency.
    pub unit_cost: Option<UnitCost>,
    pub min_order_quantity: Option<u32>,
    /// Overrides the supplier's.
    pub lead_time_days: Option<u32>,
}

/// A variant a supplier sells, and on what terms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupplierVariant {
    supplier: SupplierId,
    variant: VariantId,
    supplier_sku: Option<String>,
    unit_cost: Option<UnitCost>,
    min_order_quantity: Option<u32>,
    lead_time_days: Option<u32>,
}

impl SupplierVariant {
    /// A row for `supplier`, whose currency is `currency`: a cost in another is refused,
    /// so a row's cost is never read in the wrong currency.
    pub fn new(
        supplier: SupplierId,
        currency: Currency,
        variant: VariantId,
        draft: SupplierVariantDraft,
    ) -> Result<Self, PurchasingError> {
        if let Some(cost) = draft.unit_cost
            && cost.currency() != currency
        {
            return Err(PurchasingError::CostCurrencyMismatch {
                expected: currency,
                got: cost.currency(),
            });
        }
        Self::checked(supplier, variant, draft)
    }

    fn checked(
        supplier: SupplierId,
        variant: VariantId,
        draft: SupplierVariantDraft,
    ) -> Result<Self, PurchasingError> {
        if draft.min_order_quantity == Some(0) {
            return Err(PurchasingError::InvalidMinOrderQuantity);
        }
        if let Some(days) = draft.lead_time_days {
            check_lead_time(days)?;
        }
        Ok(Self {
            supplier,
            variant,
            supplier_sku: optional_text("supplier SKU", draft.supplier_sku)?,
            unit_cost: draft.unit_cost,
            min_order_quantity: draft.min_order_quantity,
            lead_time_days: draft.lead_time_days,
        })
    }

    /// Rebuilds a row from storage, checked like a new one but for the currency, which
    /// the supplier's own rule keeps in step.
    pub fn restore(
        supplier: SupplierId,
        variant: VariantId,
        draft: SupplierVariantDraft,
    ) -> Result<Self, PurchasingError> {
        Self::checked(supplier, variant, draft)
    }

    pub fn supplier(&self) -> SupplierId {
        self.supplier
    }

    pub fn variant(&self) -> VariantId {
        self.variant
    }

    pub fn supplier_sku(&self) -> Option<&str> {
        self.supplier_sku.as_deref()
    }

    pub fn unit_cost(&self) -> Option<UnitCost> {
        self.unit_cost
    }

    pub fn min_order_quantity(&self) -> Option<u32> {
        self.min_order_quantity
    }

    pub fn lead_time_days(&self) -> Option<u32> {
        self.lead_time_days
    }

    /// The row as a draft, for storage.
    pub fn draft(&self) -> SupplierVariantDraft {
        SupplierVariantDraft {
            supplier_sku: self.supplier_sku.clone(),
            unit_cost: self.unit_cost,
            min_order_quantity: self.min_order_quantity,
            lead_time_days: self.lead_time_days,
        }
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;

    fn sup() -> SupplierId {
        SupplierId::from_uuid(Uuid::from_u128(1))
    }

    fn var() -> VariantId {
        VariantId::from_uuid(Uuid::from_u128(2))
    }

    #[test]
    fn a_cost_is_in_the_suppliers_currency() {
        let usd = SupplierVariantDraft {
            unit_cost: Some(UnitCost::parse("350", Currency::USD).unwrap()),
            ..SupplierVariantDraft::default()
        };
        assert_eq!(
            SupplierVariant::new(sup(), Currency::EUR, var(), usd.clone()),
            Err(PurchasingError::CostCurrencyMismatch {
                expected: Currency::EUR,
                got: Currency::USD
            })
        );
        assert!(SupplierVariant::new(sup(), Currency::USD, var(), usd).is_ok());
    }

    #[test]
    fn a_minimum_order_is_at_least_one() {
        let draft = |moq| SupplierVariantDraft {
            min_order_quantity: Some(moq),
            ..SupplierVariantDraft::default()
        };
        assert!(SupplierVariant::new(sup(), Currency::EUR, var(), draft(1)).is_ok());
        assert_eq!(
            SupplierVariant::new(sup(), Currency::EUR, var(), draft(0)),
            Err(PurchasingError::InvalidMinOrderQuantity)
        );
    }

    #[test]
    fn the_sku_is_trimmed_and_blank_is_none() {
        let draft = |sku: &str| SupplierVariantDraft {
            supplier_sku: Some(sku.to_string()),
            ..SupplierVariantDraft::default()
        };
        let row = SupplierVariant::new(sup(), Currency::EUR, var(), draft(" SL-1 ")).unwrap();
        assert_eq!(row.supplier_sku(), Some("SL-1"));
        let row = SupplierVariant::new(sup(), Currency::EUR, var(), draft("  ")).unwrap();
        assert_eq!(row.supplier_sku(), None);
    }

    #[test]
    fn a_lead_time_is_at_most_a_year() {
        let draft = SupplierVariantDraft {
            lead_time_days: Some(366),
            ..SupplierVariantDraft::default()
        };
        assert_eq!(
            SupplierVariant::new(sup(), Currency::EUR, var(), draft),
            Err(PurchasingError::LeadTimeTooLong {
                days: 366,
                max: 365
            })
        );
    }
}
