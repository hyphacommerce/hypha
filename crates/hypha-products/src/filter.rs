//! What a product list is narrowed to (design notes 0012 and 0019, SE8): the admin's
//! list, read straight from the products, so staff see drafts and their own edits at
//! once. Shoppers search the index instead (`hypha-search`).

use hypha_core::optional_text;

use crate::{Product, ProductError, ProductStatus};

/// A list's filters, checked. Every one given must match.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProductFilter {
    status: Option<ProductStatus>,
    q: Option<ProductQuery>,
}

impl ProductFilter {
    /// `q` follows the shared text rule (trimmed, at most 255 characters); blank is no
    /// text filter.
    pub fn new(status: Option<ProductStatus>, q: Option<String>) -> Result<Self, ProductError> {
        Ok(Self {
            status,
            q: optional_text("q", q)?.map(ProductQuery),
        })
    }

    pub fn status(&self) -> Option<ProductStatus> {
        self.status
    }

    pub fn q(&self) -> Option<&ProductQuery> {
        self.q.as_ref()
    }

    /// Whether `product` is in the list. Adapters that filter in storage must agree
    /// (the repository contract checks).
    pub fn matches(&self, product: &Product) -> bool {
        self.status.is_none_or(|s| product.status() == s)
            && self.q.as_ref().is_none_or(|q| q.matches(product))
    }
}

/// Text a product's title, handle or one of its SKUs contains, ignoring case (Unicode
/// lowercase on both sides; a database's `lower` may differ on rare characters).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductQuery(String);

impl ProductQuery {
    /// As the merchant typed it, trimmed.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn matches(&self, product: &Product) -> bool {
        let q = self.0.to_lowercase();
        let has = |text: &str| text.to_lowercase().contains(&q);
        has(product.title())
            || has(product.handle().as_str())
            || product.variants().iter().any(|v| v.sku().is_some_and(has))
    }
}

#[cfg(test)]
mod tests {
    use hypha_core::{Currency, Money, Price, ProductId, TextError, VariantId};
    use uuid::Uuid;

    use super::*;
    use crate::{Handle, VariantPatch};

    fn mug() -> Product {
        let mut p = Product::new_simple(
            ProductId::from_uuid(Uuid::from_u128(1)),
            Handle::new("stoneware-mug").unwrap(),
            "Čaša Mug".to_string(),
            VariantId::from_uuid(Uuid::from_u128(2)),
            Price::new(Money::new(1000, Currency::EUR).unwrap()).unwrap(),
        )
        .unwrap();
        p.update_variant(
            VariantId::from_uuid(Uuid::from_u128(2)),
            VariantPatch {
                sku: Some(Some("MUG-01".to_string())),
                ..VariantPatch::default()
            },
        )
        .unwrap();
        p
    }

    fn q(text: &str) -> ProductFilter {
        ProductFilter::new(None, Some(text.to_string())).unwrap()
    }

    #[test]
    fn text_matches_title_handle_or_sku_ignoring_case() {
        let p = mug();
        for text in ["čAŠA", "ware-m", "mug-01", "Mug"] {
            assert!(q(text).matches(&p), "{text}");
        }
        assert!(!q("cup").matches(&p));
    }

    #[test]
    fn blank_text_filters_nothing() {
        assert_eq!(
            ProductFilter::new(None, Some("  ".to_string())),
            Ok(ProductFilter::default())
        );
    }

    #[test]
    fn text_follows_the_text_rule() {
        assert_eq!(
            ProductFilter::new(None, Some("x".repeat(256))),
            Err(ProductError::Text(TextError::TooLong("q")))
        );
    }

    #[test]
    fn status_and_text_must_both_match() {
        let p = mug();
        let draft_mugs = ProductFilter::new(Some(ProductStatus::Draft), Some("mug".to_string()));
        let active_mugs = ProductFilter::new(Some(ProductStatus::Active), Some("mug".to_string()));
        assert!(draft_mugs.unwrap().matches(&p));
        assert!(!active_mugs.unwrap().matches(&p));
    }
}
