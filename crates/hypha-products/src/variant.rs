use hypha_core::{OptionValueId, Price, VariantId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variant {
    id: VariantId,
    sku: Option<String>,
    base_price: Price,
    selections: Vec<OptionValueId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewVariant {
    pub id: VariantId,
    pub sku: Option<String>,
    pub price: Price,
    pub selections: Vec<OptionValueId>,
}

impl Variant {
    pub(crate) fn new(
        id: VariantId,
        sku: Option<String>,
        base_price: Price,
        selections: Vec<OptionValueId>,
    ) -> Self {
        Self {
            id,
            sku,
            base_price,
            selections,
        }
    }

    pub fn id(&self) -> VariantId {
        self.id
    }
    pub fn base_price(&self) -> Price {
        self.base_price
    }
    pub fn sku(&self) -> Option<&str> {
        self.sku.as_deref()
    }
    pub fn selections(&self) -> &[OptionValueId] {
        &self.selections
    }
}
