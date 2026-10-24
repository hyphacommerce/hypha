//! What a product list is narrowed to: the admin's list, read straight from the products,
//! so staff see drafts and their own edits at once. Shoppers search the index instead
//! (`hypha-search`).

use std::{fmt, str::FromStr};

use hypha_core::{
    CategoryId, ChannelId, Direction, FieldFilter, ProductId, SortKey, Timestamp, VariantId,
    optional_text,
};

use crate::{Product, ProductError, ProductStatus, ProductSummary, Variant, VariantSummary};

/// How many product IDs a product list may be narrowed to (`ids`).
pub const MAX_FILTER_PRODUCTS: usize = 250;
/// How many products a variant list may be narrowed to: each may have many variants.
pub const MAX_FILTER_VARIANT_PRODUCTS: usize = 100;
/// How many variant IDs a variant list may be narrowed to.
pub const MAX_FILTER_VARIANTS: usize = 250;

/// A list's filters, checked. Every one given must match.
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
    /// Any of these; empty is every product.
    ids: Vec<ProductId>,
    fields: Vec<FieldFilter>,
}

/// What a product list may be narrowed to, unchecked (`ProductFilter::new` checks it).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProductFilterInput {
    /// Any of these; empty is every status.
    pub statuses: Vec<ProductStatus>,
    /// Text the title, handle or a SKU contains, ignoring case.
    pub q: Option<String>,
    /// The vendor, product type or a tag, each matched whole, ignoring case.
    pub vendor: Option<String>,
    pub product_type: Option<String>,
    pub tag: Option<String>,
    /// Products directly in this category. Stored by the categories domain: an ID that
    /// names none matches nothing.
    pub category: Option<CategoryId>,
    /// Products published to this channel; the same.
    pub channel: Option<ChannelId>,
    /// Any of these products (at most `MAX_FILTER_PRODUCTS`); empty is every product.
    pub ids: Vec<ProductId>,
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
            ids: id_set("ids", input.ids, MAX_FILTER_PRODUCTS)?,
            fields: Vec::new(),
        })
    }

    /// Filters on custom fields, checked against their definitions by `hypha-custom-fields`
    /// already; every one must match. `matches` doesn't see them: adapters apply them to
    /// the stored values.
    pub fn with_fields(mut self, fields: Vec<FieldFilter>) -> Self {
        self.fields = fields;
        self
    }

    pub fn fields(&self) -> &[FieldFilter] {
        &self.fields
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

    /// Only these products (sorted, no repeats); empty is all of them.
    pub fn ids(&self) -> &[ProductId] {
        &self.ids
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
            && (self.ids.is_empty() || self.ids.contains(&product.id()))
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

/// `ids` sorted without repeats, refused past `max`.
fn id_set<Id: Ord>(
    field: &'static str,
    mut ids: Vec<Id>,
    max: usize,
) -> Result<Vec<Id>, ProductError> {
    ids.sort();
    ids.dedup();
    if ids.len() > max {
        return Err(ProductError::TooManyFilterIds { field, max });
    }
    Ok(ids)
}

/// A variant list's filters, checked. Every one given must match.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VariantFilter {
    q: Option<String>,
    products: Vec<ProductId>,
    category: Option<CategoryId>,
    ids: Vec<VariantId>,
}

/// What a variant list may be narrowed to, unchecked (`VariantFilter::new` checks it).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VariantFilterInput {
    /// Text the product's title or handle, or the variant's own SKU, contains, ignoring
    /// case.
    pub q: Option<String>,
    /// Variants of any of these products (at most `MAX_FILTER_VARIANT_PRODUCTS`); empty is
    /// every product.
    pub products: Vec<ProductId>,
    /// Variants of products in this category or any below it. Stored by the categories
    /// domain: an ID that names none matches nothing.
    pub category: Option<CategoryId>,
    /// Any of these variants (at most `MAX_FILTER_VARIANTS`); empty is every variant.
    pub ids: Vec<VariantId>,
}

impl VariantFilter {
    /// Text follows the shared text rule (trimmed, at most 255 characters); blank is no
    /// filter.
    pub fn new(input: VariantFilterInput) -> Result<Self, ProductError> {
        Ok(Self {
            q: optional_text("q", input.q)?,
            products: id_set("products", input.products, MAX_FILTER_VARIANT_PRODUCTS)?,
            category: input.category,
            ids: id_set("ids", input.ids, MAX_FILTER_VARIANTS)?,
        })
    }

    pub fn q(&self) -> Option<&str> {
        self.q.as_deref()
    }

    /// Sorted, no repeats; empty is all of them.
    pub fn products(&self) -> &[ProductId] {
        &self.products
    }

    pub fn category(&self) -> Option<CategoryId> {
        self.category
    }

    /// Sorted, no repeats; empty is all of them.
    pub fn ids(&self) -> &[VariantId] {
        &self.ids
    }

    /// Whether `product`'s `variant` is in the list, given whether the product is in the
    /// filter's category or one below it (asked only when the filter has one). Adapters
    /// that filter in storage must agree (the repository contract checks).
    pub fn matches(
        &self,
        product: &Product,
        variant: &Variant,
        in_category: impl Fn(CategoryId) -> bool,
    ) -> bool {
        let text = |q: &str| {
            let q = q.to_lowercase();
            let has = |text: &str| text.to_lowercase().contains(&q);
            has(product.title()) || has(product.handle().as_str()) || variant.sku().is_some_and(has)
        };
        (self.products.is_empty() || self.products.contains(&product.id()))
            && (self.ids.is_empty() || self.ids.contains(&variant.id()))
            && self.q.as_deref().is_none_or(text)
            && self.category.is_none_or(in_category)
    }
}

/// Where a variant list is: the newest product first, each product's variants in their
/// order. A page continues after one of these; written `prod_…/3` in a cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VariantPosition {
    pub product: ProductId,
    /// The variant's index in its product's variants.
    pub position: u32,
}

impl VariantPosition {
    /// Whether the variant at `other` comes after this position in the list.
    pub fn precedes(&self, other: VariantPosition) -> bool {
        other.product < self.product
            || (other.product == self.product && other.position > self.position)
    }
}

impl VariantSummary {
    pub fn at(&self) -> VariantPosition {
        VariantPosition {
            product: self.product,
            position: self.position,
        }
    }
}

impl fmt::Display for VariantPosition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.product, self.position)
    }
}

impl FromStr for VariantPosition {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (product, position) = s.split_once('/').ok_or(())?;
        Ok(Self {
            product: product.parse().map_err(|_| ())?,
            position: position.parse().map_err(|_| ())?,
        })
    }
}

/// How a product list is ordered. Titles compare as `SortKey::text` does; ties go by ID in
/// the same direction.
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
    fn filter_ids_are_capped_counted_without_repeats() {
        let ids = |n: u128| (1..=n).map(|i| ProductId::from_uuid(Uuid::from_u128(i)));
        let mut at_limit: Vec<ProductId> = ids(250).collect();
        at_limit.push(ProductId::from_uuid(Uuid::from_u128(1)));
        let filter = ProductFilter::new(ProductFilterInput {
            ids: at_limit,
            ..ProductFilterInput::default()
        })
        .unwrap();
        assert_eq!(filter.ids().len(), 250);
        assert_eq!(
            ProductFilter::new(ProductFilterInput {
                ids: ids(251).collect(),
                ..ProductFilterInput::default()
            }),
            Err(ProductError::TooManyFilterIds {
                field: "ids",
                max: 250
            })
        );
        assert_eq!(
            VariantFilter::new(VariantFilterInput {
                products: ids(101).collect(),
                ..VariantFilterInput::default()
            }),
            Err(ProductError::TooManyFilterIds {
                field: "products",
                max: 100
            })
        );
        assert!(
            VariantFilter::new(VariantFilterInput {
                products: ids(100).collect(),
                ..VariantFilterInput::default()
            })
            .is_ok()
        );
    }

    #[test]
    fn listed_ids_match_only_those_products() {
        let p = mug();
        let by = |n: u128| {
            filter(ProductFilterInput {
                ids: vec![ProductId::from_uuid(Uuid::from_u128(n))],
                ..ProductFilterInput::default()
            })
        };
        assert!(matches(&by(1), &p));
        assert!(!matches(&by(2), &p));
    }

    #[test]
    fn variant_text_matches_the_title_handle_or_sku_ignoring_case() {
        let p = mug();
        let q = |text: &str| {
            VariantFilter::new(VariantFilterInput {
                q: Some(text.to_string()),
                ..VariantFilterInput::default()
            })
            .unwrap()
        };
        let variant = &p.variants()[0];
        for text in ["čAŠA", "ware-m", "mug-01"] {
            assert!(q(text).matches(&p, variant, |_| panic!()), "{text}");
        }
        assert!(!q("cup").matches(&p, variant, |_| panic!()));
    }

    #[test]
    fn a_variant_position_continues_within_the_product_then_older_ones() {
        let at = |p: u128, position| VariantPosition {
            product: ProductId::from_uuid(Uuid::from_u128(p)),
            position,
        };
        assert!(at(5, 1).precedes(at(5, 2)));
        assert!(!at(5, 1).precedes(at(5, 1)), "not itself");
        assert!(!at(5, 1).precedes(at(5, 0)));
        assert!(at(5, 1).precedes(at(4, 0)));
        assert!(!at(5, 1).precedes(at(6, 9)));
    }

    #[test]
    fn a_variant_position_reads_back_from_its_text() {
        let at = VariantPosition {
            product: ProductId::from_uuid(Uuid::from_u128(7)),
            position: 12,
        };
        assert_eq!(at.to_string().parse(), Ok(at));
        assert_eq!("prod_x/1".parse::<VariantPosition>(), Err(()));
        assert_eq!(
            format!("{}/-1", at.product).parse::<VariantPosition>(),
            Err(())
        );
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
