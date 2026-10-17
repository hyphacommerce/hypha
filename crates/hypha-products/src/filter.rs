//! What a product list is narrowed to (design notes 0012 and 0019, SE8): the admin's
//! list, read straight from the products, so staff see drafts and their own edits at
//! once. Shoppers search the index instead (`hypha-search`).

use hypha_core::{CategoryId, ChannelId, Direction, SortKey, Timestamp, optional_text};

use crate::{Product, ProductError, ProductStatus, ProductSummary};

/// A list's filters, checked. Every one given must match (design note 0024, AU4).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProductFilter {
    /// Any of these; empty is every status.
    statuses: Vec<ProductStatus>,
    q: Option<ProductQuery>,
    vendor: Option<String>,
    product_type: Option<String>,
    tag: Option<String>,
    category: Option<CategoryId>,
    channel: Option<ChannelId>,
}

/// What a product list may be narrowed to, unchecked (`ProductFilter::new` checks it).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProductFilterInput {
    /// Any of these; empty is every status.
    pub statuses: Vec<ProductStatus>,
    /// Text the title, handle or a SKU contains, ignoring case (design note 0019, SE8).
    pub q: Option<String>,
    /// The vendor, product type or a tag, each matched whole, ignoring case.
    pub vendor: Option<String>,
    pub product_type: Option<String>,
    pub tag: Option<String>,
    /// Products directly in this category (design note 0022). Stored by the categories
    /// domain: an ID that names none matches nothing (design note 0024, AU5).
    pub category: Option<CategoryId>,
    /// Products published to this channel (design note 0020); the same.
    pub channel: Option<ChannelId>,
}

impl ProductFilter {
    /// Text follows the shared text rule (trimmed, at most 255 characters); blank is no
    /// filter.
    pub fn new(input: ProductFilterInput) -> Result<Self, ProductError> {
        let mut statuses = input.statuses;
        statuses.sort_by_key(|s| s.as_str());
        statuses.dedup();
        Ok(Self {
            statuses,
            q: optional_text("q", input.q)?.map(ProductQuery),
            vendor: optional_text("vendor", input.vendor)?,
            product_type: optional_text("product_type", input.product_type)?,
            tag: optional_text("tag", input.tag)?,
            category: input.category,
            channel: input.channel,
        })
    }

    /// Only these statuses (sorted, no repeats); empty is all of them.
    pub fn statuses(&self) -> &[ProductStatus] {
        &self.statuses
    }

    pub fn q(&self) -> Option<&ProductQuery> {
        self.q.as_ref()
    }

    pub fn vendor(&self) -> Option<&str> {
        self.vendor.as_deref()
    }

    pub fn product_type(&self) -> Option<&str> {
        self.product_type.as_deref()
    }

    pub fn tag(&self) -> Option<&str> {
        self.tag.as_deref()
    }

    pub fn category(&self) -> Option<CategoryId> {
        self.category
    }

    pub fn channel(&self) -> Option<ChannelId> {
        self.channel
    }

    /// Whether `product` is in the list, given whether it is in the filter's category
    /// and channel (asked only when the filter has them: membership is stored by other
    /// domains). Adapters that filter in storage must agree (the repository contract
    /// checks).
    pub fn matches(
        &self,
        product: &Product,
        in_category: impl Fn(CategoryId) -> bool,
        in_channel: impl Fn(ChannelId) -> bool,
    ) -> bool {
        let same = |wanted: &Option<String>, value: Option<&str>| {
            wanted
                .as_ref()
                .is_none_or(|w| value.is_some_and(|v| v.to_lowercase() == w.to_lowercase()))
        };
        (self.statuses.is_empty() || self.statuses.contains(&product.status()))
            && self.q.as_ref().is_none_or(|q| q.matches(product))
            && same(&self.vendor, product.vendor())
            && same(&self.product_type, product.product_type())
            && self.tag.as_ref().is_none_or(|t| {
                let t = t.to_lowercase();
                product.tags().iter().any(|tag| tag.to_lowercase() == t)
            })
            && self.category.is_none_or(in_category)
            && self.channel.is_none_or(in_channel)
    }
}

/// How a product list is ordered (design note 0024, AU1). Titles compare as
/// `SortKey::text` does; ties go by ID in the same direction.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ProductSort {
    /// Newest first: creation order, as lists were before sorting.
    #[default]
    Newest,
    Oldest,
    TitleAsc,
    TitleDesc,
    /// Most recently changed first.
    UpdatedDesc,
    UpdatedAsc,
}

impl ProductSort {
    pub fn direction(self) -> Direction {
        match self {
            ProductSort::Oldest | ProductSort::TitleAsc | ProductSort::UpdatedAsc => {
                Direction::Ascending
            }
            ProductSort::Newest | ProductSort::TitleDesc | ProductSort::UpdatedDesc => {
                Direction::Descending
            }
        }
    }

    /// The key `summary` sorts by. A summary without stamps (never stored) sorts as the
    /// epoch.
    pub fn key(self, summary: &ProductSummary) -> SortKey {
        match self {
            ProductSort::Newest | ProductSort::Oldest => SortKey::Id,
            ProductSort::TitleAsc | ProductSort::TitleDesc => SortKey::text(&summary.title),
            ProductSort::UpdatedDesc | ProductSort::UpdatedAsc => SortKey::Time(
                summary
                    .stamps
                    .map_or(Timestamp::UNIX_EPOCH, |s| s.updated_at),
            ),
        }
    }

    /// Whether `key` is the kind this sort uses: a cursor made for another sort doesn't.
    pub fn fits(self, key: &SortKey) -> bool {
        matches!(
            (self, key),
            (ProductSort::Newest | ProductSort::Oldest, SortKey::Id)
                | (
                    ProductSort::TitleAsc | ProductSort::TitleDesc,
                    SortKey::Text(_)
                )
                | (
                    ProductSort::UpdatedDesc | ProductSort::UpdatedAsc,
                    SortKey::Time(_)
                )
        )
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

    fn filter(input: ProductFilterInput) -> ProductFilter {
        ProductFilter::new(input).unwrap()
    }

    fn q(text: &str) -> ProductFilter {
        filter(ProductFilterInput {
            q: Some(text.to_string()),
            ..ProductFilterInput::default()
        })
    }

    fn matches(filter: &ProductFilter, product: &Product) -> bool {
        filter.matches(product, |_| false, |_| false)
    }

    #[test]
    fn text_matches_title_handle_or_sku_ignoring_case() {
        let p = mug();
        for text in ["čAŠA", "ware-m", "mug-01", "Mug"] {
            assert!(matches(&q(text), &p), "{text}");
        }
        assert!(!matches(&q("cup"), &p));
    }

    #[test]
    fn blank_text_filters_nothing() {
        assert_eq!(
            ProductFilter::new(ProductFilterInput {
                q: Some("  ".to_string()),
                vendor: Some(" ".to_string()),
                ..ProductFilterInput::default()
            }),
            Ok(ProductFilter::default())
        );
    }

    #[test]
    fn text_follows_the_text_rule() {
        assert_eq!(
            ProductFilter::new(ProductFilterInput {
                q: Some("x".repeat(256)),
                ..ProductFilterInput::default()
            }),
            Err(ProductError::Text(TextError::TooLong("q")))
        );
        assert_eq!(
            ProductFilter::new(ProductFilterInput {
                tag: Some("x".repeat(256)),
                ..ProductFilterInput::default()
            }),
            Err(ProductError::Text(TextError::TooLong("tag")))
        );
    }

    #[test]
    fn status_and_text_must_both_match() {
        let p = mug();
        let mugs = |status| {
            filter(ProductFilterInput {
                statuses: vec![status],
                q: Some("mug".to_string()),
                ..ProductFilterInput::default()
            })
        };
        assert!(matches(&mugs(ProductStatus::Draft), &p));
        assert!(!matches(&mugs(ProductStatus::Active), &p));
    }

    #[test]
    fn any_of_several_statuses_matches() {
        let p = mug();
        let either = filter(ProductFilterInput {
            statuses: vec![ProductStatus::Active, ProductStatus::Draft],
            ..ProductFilterInput::default()
        });
        assert!(matches(&either, &p));
        assert_eq!(
            either.statuses(),
            filter(ProductFilterInput {
                statuses: vec![
                    ProductStatus::Draft,
                    ProductStatus::Active,
                    ProductStatus::Draft
                ],
                ..ProductFilterInput::default()
            })
            .statuses()
        );
    }

    #[test]
    fn vendor_type_and_tag_match_whole_ignoring_case() {
        let mut p = mug();
        p.set_vendor(Some("Acme Ceramics".to_string())).unwrap();
        p.set_product_type(Some("Mug".to_string())).unwrap();
        p.set_tags(vec!["Gift".to_string(), "kitchen".to_string()])
            .unwrap();
        let by = |input| matches(&filter(input), &p);
        assert!(by(ProductFilterInput {
            vendor: Some("acme ceramics".to_string()),
            product_type: Some("MUG".to_string()),
            tag: Some("gift".to_string()),
            ..ProductFilterInput::default()
        }));
        assert!(!by(ProductFilterInput {
            vendor: Some("Acme".to_string()),
            ..ProductFilterInput::default()
        }));
        assert!(!by(ProductFilterInput {
            tag: Some("kitch".to_string()),
            ..ProductFilterInput::default()
        }));
    }

    #[test]
    fn category_and_channel_are_asked_only_when_filtered() {
        let p = mug();
        let pcat = CategoryId::from_uuid(Uuid::from_u128(9));
        let chan = ChannelId::from_uuid(Uuid::from_u128(8));
        assert!(ProductFilter::default().matches(&p, |_| panic!(), |_| panic!()));
        let both = filter(ProductFilterInput {
            category: Some(pcat),
            channel: Some(chan),
            ..ProductFilterInput::default()
        });
        assert!(both.matches(&p, |c| c == pcat, |c| c == chan));
        assert!(!both.matches(&p, |c| c == pcat, |_| false));
    }

    #[test]
    fn each_sort_fits_only_its_own_key() {
        assert!(ProductSort::Newest.fits(&SortKey::Id));
        assert!(ProductSort::TitleDesc.fits(&SortKey::text("a")));
        assert!(!ProductSort::TitleAsc.fits(&SortKey::Id));
        assert!(!ProductSort::UpdatedAsc.fits(&SortKey::text("a")));
        assert!(ProductSort::UpdatedAsc.fits(&SortKey::Time(Timestamp::UNIX_EPOCH)));
    }

    #[test]
    fn titles_sort_ignoring_case() {
        let summary = mug().summary();
        assert_eq!(
            ProductSort::TitleAsc.key(&summary),
            SortKey::text("čaša mug")
        );
        assert_eq!(ProductSort::Oldest.key(&summary), SortKey::Id);
    }
}
