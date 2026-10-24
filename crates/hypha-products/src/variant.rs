use hypha_core::{AssetId, OptionValueId, Price, VariantId, Weight};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variant {
    id: VariantId,
    sku: Option<String>,
    base_price: Price,
    /// The merchant's "was" price, struck through beside the base price; above it, in its
    /// currency (`Product::update_variant` checks).
    compare_at_price: Option<Price>,
    selections: Vec<OptionValueId>,
    media: Option<AssetId>,
    /// A fact about the product, read by shipping; `None` when not entered.
    weight: Option<Weight>,
    /// `false` for what is never shipped (downloads, gift cards).
    requires_shipping: bool,
}

/// A new variant has no weight and requires shipping; `VariantPatch` changes either.
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
    /// `Some(None)` clears the compare-at price.
    pub compare_at_price: Option<Option<Price>>,
    pub selections: Option<Vec<OptionValueId>>,
    /// `Some(None)` clears the weight.
    pub weight: Option<Option<Weight>>,
    pub requires_shipping: Option<bool>,
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
            compare_at_price: None,
            selections,
            media: None,
            weight: None,
            requires_shipping: true,
        }
    }

    pub fn id(&self) -> VariantId {
        self.id
    }
    pub fn base_price(&self) -> Price {
        self.base_price
    }
    pub fn compare_at_price(&self) -> Option<Price> {
        self.compare_at_price
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
    pub fn weight(&self) -> Option<Weight> {
        self.weight
    }
    pub fn requires_shipping(&self) -> bool {
        self.requires_shipping
    }

    pub(crate) fn set_sku(&mut self, sku: Option<String>) {
        self.sku = sku;
    }
    pub(crate) fn set_base_price(&mut self, price: Price) {
        self.base_price = price;
    }
    pub(crate) fn set_compare_at_price(&mut self, price: Option<Price>) {
        self.compare_at_price = price;
    }
    pub(crate) fn set_selections(&mut self, selections: Vec<OptionValueId>) {
        self.selections = selections;
    }
    pub(crate) fn set_media(&mut self, media: Option<AssetId>) {
        self.media = media;
    }
    pub(crate) fn set_weight(&mut self, weight: Option<Weight>) {
        self.weight = weight;
    }
    pub(crate) fn set_requires_shipping(&mut self, requires: bool) {
        self.requires_shipping = requires;
    }
}
