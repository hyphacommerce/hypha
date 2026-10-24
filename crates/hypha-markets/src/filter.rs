//! What lists of price lists and of fixed prices may be narrowed to, and their orders.

use hypha_core::{
    CategoryId, CustomerGroupId, Direction, PriceListId, ProductId, SortKey, Timestamp,
    optional_text,
};

use crate::{PriceList, PriceListError, PriceListKind, PriceListPhase};

/// Price lists of a kind, in a phase at a moment, among some IDs, for a customer group;
/// `None` filters nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriceListFilter {
    pub kind: Option<PriceListKind>,
    /// The phase lists must be in at `at`.
    pub phase: Option<PriceListPhase>,
    pub at: Timestamp,
    /// Only these lists (a market's catalog's, say); empty matches none.
    pub ids: Option<Vec<PriceListId>>,
    /// Only lists whose audience names this group.
    pub group: Option<CustomerGroupId>,
}

impl PriceListFilter {
    /// Every list.
    pub fn everything(at: Timestamp) -> Self {
        Self {
            kind: None,
            phase: None,
            at,
            ids: None,
            group: None,
        }
    }

    /// What adapters must return: the rule in one place, for the in-memory one and tests.
    pub fn matches(&self, list: &PriceList) -> bool {
        self.kind.is_none_or(|k| list.kind() == k)
            && self.phase.is_none_or(|p| list.phase(self.at) == p)
            && self.ids.as_ref().is_none_or(|ids| ids.contains(&list.id()))
            && self
                .group
                .is_none_or(|g| list.audience().groups().contains(&g))
    }
}

/// How price lists are ordered; names compare as `SortKey::text` does, ties by ID in the
/// same direction.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PriceListSort {
    #[default]
    Newest,
    Oldest,
    NameAsc,
    NameDesc,
}

impl PriceListSort {
    pub fn direction(self) -> Direction {
        match self {
            PriceListSort::Oldest | PriceListSort::NameAsc => Direction::Ascending,
            PriceListSort::Newest | PriceListSort::NameDesc => Direction::Descending,
        }
    }

    pub fn key(self, list: &PriceList) -> SortKey {
        match self {
            PriceListSort::Newest | PriceListSort::Oldest => SortKey::Id,
            PriceListSort::NameAsc | PriceListSort::NameDesc => SortKey::text(list.name()),
        }
    }

    /// Whether `key` is the kind this sort uses: a cursor made for another sort doesn't.
    pub fn fits(self, key: &SortKey) -> bool {
        matches!(
            (self, key),
            (PriceListSort::Newest | PriceListSort::Oldest, SortKey::Id)
                | (
                    PriceListSort::NameAsc | PriceListSort::NameDesc,
                    SortKey::Text(_)
                )
        )
    }
}

/// A list's fixed prices for one product, the products directly in one category, or
/// those whose title, handle or a SKU contains text (ignoring case). Products and
/// categories are other domains' records, which adapters join: an ID that names none
/// matches nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FixedPriceFilter {
    product: Option<ProductId>,
    category: Option<CategoryId>,
    q: Option<String>,
}

impl FixedPriceFilter {
    /// Text follows the shared text rule (trimmed, at most 255 characters); blank is no
    /// filter.
    pub fn new(
        product: Option<ProductId>,
        category: Option<CategoryId>,
        q: Option<String>,
    ) -> Result<Self, PriceListError> {
        Ok(Self {
            product,
            category,
            q: optional_text("q", q)?,
        })
    }

    pub fn product(&self) -> Option<ProductId> {
        self.product
    }

    pub fn category(&self) -> Option<CategoryId> {
        self.category
    }

    /// The text, as given (trimmed).
    pub fn q(&self) -> Option<&str> {
        self.q.as_deref()
    }

    /// Whether it narrows anything.
    pub fn is_empty(&self) -> bool {
        self.product.is_none() && self.category.is_none() && self.q.is_none()
    }

    /// Whether a product passes: `in_category` says whether it's directly in the
    /// filter's category, `texts` are its title, handle and the variant's SKU.
    pub fn matches<'a>(
        &self,
        product: ProductId,
        in_category: bool,
        texts: impl IntoIterator<Item = &'a str>,
    ) -> bool {
        if self.product.is_some_and(|p| p != product) {
            return false;
        }
        if self.category.is_some() && !in_category {
            return false;
        }
        match &self.q {
            None => true,
            Some(q) => {
                let q = q.to_lowercase();
                texts.into_iter().any(|t| t.to_lowercase().contains(&q))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use hypha_core::{Currency, TextError};
    use uuid::Uuid;

    use super::*;
    use crate::PriceListDraft;

    fn list(name: &str) -> PriceList {
        PriceList::new(
            PriceListId::from_uuid(Uuid::from_u128(1)),
            PriceListDraft::new(name, Currency::EUR),
        )
        .unwrap()
    }

    #[test]
    fn each_sort_fits_only_its_own_key() {
        assert!(PriceListSort::Oldest.fits(&SortKey::Id));
        assert!(PriceListSort::NameDesc.fits(&SortKey::text("a")));
        assert!(!PriceListSort::NameAsc.fits(&SortKey::Id));
    }

    #[test]
    fn a_filter_matches_kind_phase_and_ids() {
        let sale = list("Sale");
        let at = Timestamp::UNIX_EPOCH;
        let mut filter = PriceListFilter::everything(at);
        assert!(filter.matches(&sale));
        filter.kind = Some(PriceListKind::Sale);
        assert!(!filter.matches(&sale));
        filter.kind = None;
        filter.phase = Some(PriceListPhase::Draft);
        assert!(!filter.matches(&sale));
        filter.phase = Some(PriceListPhase::Active);
        filter.ids = Some(vec![]);
        assert!(!filter.matches(&sale));
    }

    #[test]
    fn fixed_price_text_follows_the_text_rule() {
        assert_eq!(
            FixedPriceFilter::new(None, None, Some("x".repeat(256))),
            Err(PriceListError::Text(TextError::TooLong("q")))
        );
        assert!(
            FixedPriceFilter::new(None, None, Some(" ".into()))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn fixed_price_text_matches_ignoring_case() {
        let p = ProductId::from_uuid(Uuid::from_u128(1));
        let f = FixedPriceFilter::new(None, None, Some("MUG".into())).unwrap();
        assert!(f.matches(p, false, ["Blue mug"]));
        assert!(!f.matches(p, false, ["Cup", "CUP-1"]));
    }
}
