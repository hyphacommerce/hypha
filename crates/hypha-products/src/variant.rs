use hypha_core::{AssetId, OptionValueId, Price, VariantId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variant {
    id: VariantId,
    sku: Option<String>,
    base_price: Price,
    selections: Vec<OptionValueId>,
    media: Option<AssetId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewVariant {
    pub id: VariantId,
    pub sku: Option<String>,
    pub price: Price,
    pub selections: Vec<OptionValueId>,
}

/// Changes to a variant; `None` keeps a field. `sku: Some(None)` clears the SKU.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VariantPatch {
    pub sku: Option<Option<String>>,
    pub price: Option<Price>,
    pub selections: Option<Vec<OptionValueId>>,
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
            media: None,
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
    /// The item of the product's media list this variant shows, if any.
    pub fn media(&self) -> Option<AssetId> {
        self.media
    }

    pub(crate) fn set_sku(&mut self, sku: Option<String>) {
        self.sku = sku;
    }
    pub(crate) fn set_base_price(&mut self, price: Price) {
        self.base_price = price;
    }
    pub(crate) fn set_selections(&mut self, selections: Vec<OptionValueId>) {
        self.selections = selections;
    }
    pub(crate) fn set_media(&mut self, media: Option<AssetId>) {
        self.media = media;
    }
}
