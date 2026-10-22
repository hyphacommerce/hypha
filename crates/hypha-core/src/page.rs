//! Pages of a list: newest first, continued after the last item seen.
//!
//! IDs are UUIDv7, so ordering by ID is ordering by creation time; a page asks for the
//! items whose ID is below the last one it got (`after`). A cursor like this stays
//! correct while items are added, unlike an offset, and costs one indexed range scan.
//!
//! Lists sorted by something else page the same way by a `Keyset`: the last item's sort key
//! with its ID, which breaks ties. A list that sorts takes `PageRequest<Keyset<Id>>`.

use std::cmp::Ordering;

use thiserror::Error;

use crate::{ErrorCode, Timestamp};

/// How many items a page may hold: 1 to `MAX`, `DEFAULT` when not asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageLimit(u32);

impl PageLimit {
    pub const DEFAULT: PageLimit = PageLimit(50);
    pub const MAX: u32 = 100;

    pub fn new(limit: u32) -> Result<Self, PageError> {
        if limit == 0 || limit > Self::MAX {
            return Err(PageError::LimitOutOfRange { max: Self::MAX });
        }
        Ok(Self(limit))
    }

    pub fn get(self) -> u32 {
        self.0
    }
}

impl Default for PageLimit {
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum PageError {
    #[error("a page holds 1 to {max} items")]
    LimitOutOfRange { max: u32 },
}

impl ErrorCode for PageError {
    fn code(&self) -> &'static str {
        match self {
            PageError::LimitOutOfRange { .. } => "invalid_limit",
        }
    }
}

/// Which page: up to `limit` items, all with IDs below `after` (the last ID of the
/// previous page), newest first. `after: None` is the first page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageRequest<Id> {
    pub after: Option<Id>,
    pub limit: PageLimit,
}

impl<Id> Default for PageRequest<Id> {
    fn default() -> Self {
        Self {
            after: None,
            limit: PageLimit::DEFAULT,
        }
    }
}

/// The value a sorted list orders by, at one item: what a cursor carries besides the item's
/// ID. Each sort uses one kind.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum SortKey {
    /// Sorted by ID alone (creation order): nothing besides the ID.
    Id,
    /// Text compared by its bytes after Unicode lowercasing (titles, emails).
    Text(String),
    Time(Timestamp),
    /// A whole number, such as an amount in minor units.
    Int(i64),
}

impl SortKey {
    /// Text as text sorts compare it: Unicode lowercase, then by bytes (adapters compare
    /// `lower(…) COLLATE "C"`).
    pub fn text(text: &str) -> Self {
        SortKey::Text(text.to_lowercase())
    }
}

/// Which way a list is sorted; the ID breaks ties in the same direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Ascending,
    Descending,
}

/// Where a sorted list continues: after the item with this sort key and ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keyset<Id> {
    pub key: SortKey,
    pub id: Id,
}

impl<Id: Ord> Keyset<Id> {
    /// How two items compare in a list sorted `direction`: by key, then by ID.
    pub fn order(a: (&SortKey, &Id), b: (&SortKey, &Id), direction: Direction) -> Ordering {
        let ascending = a.0.cmp(b.0).then_with(|| a.1.cmp(b.1));
        match direction {
            Direction::Ascending => ascending,
            Direction::Descending => ascending.reverse(),
        }
    }

    /// Whether the item at `key` and `id` comes after this position.
    pub fn precedes(&self, key: &SortKey, id: &Id, direction: Direction) -> bool {
        Self::order((&self.key, &self.id), (key, id), direction) == Ordering::Less
    }
}

/// One page, newest first. `has_more` says whether a page after it has items; its
/// cursor is the last item's ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub has_more: bool,
}

impl<T> Page<T> {
    /// Builds a page from up to `limit + 1` items fetched newest first: the extra one
    /// only tells that there are more. (Adapters ask for one more than the limit.)
    pub fn from_overfetch(mut items: Vec<T>, limit: PageLimit) -> Self {
        let has_more = items.len() > limit.get() as usize;
        items.truncate(limit.get() as usize);
        Self { items, has_more }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_keyset_breaks_ties_by_id_in_the_same_direction() {
        let at = Keyset {
            key: SortKey::text("Mug"),
            id: 5,
        };
        let mug = SortKey::text("MUG");
        assert!(at.precedes(&mug, &6, Direction::Ascending));
        assert!(!at.precedes(&mug, &4, Direction::Ascending));
        assert!(at.precedes(&mug, &4, Direction::Descending));
        assert!(!at.precedes(&mug, &5, Direction::Descending), "not itself");
        assert!(at.precedes(&SortKey::text("a"), &9, Direction::Descending));
        assert!(at.precedes(&SortKey::text("z"), &1, Direction::Ascending));
    }

    #[test]
    fn text_keys_ignore_case_beyond_ascii() {
        assert_eq!(SortKey::text("ČAŠA"), SortKey::text("čaša"));
    }

    #[test]
    fn a_limit_is_1_to_100() {
        assert_eq!(
            PageLimit::new(0),
            Err(PageError::LimitOutOfRange { max: 100 })
        );
        assert_eq!(PageLimit::new(1).map(PageLimit::get), Ok(1));
        assert_eq!(PageLimit::new(100).map(PageLimit::get), Ok(100));
        assert_eq!(
            PageLimit::new(101),
            Err(PageError::LimitOutOfRange { max: 100 })
        );
    }

    #[test]
    fn an_overfetched_item_means_more() {
        let limit = PageLimit::new(2).unwrap();
        assert_eq!(
            Page::from_overfetch(vec![3, 2, 1], limit),
            Page {
                items: vec![3, 2],
                has_more: true
            }
        );
        assert_eq!(
            Page::from_overfetch(vec![3, 2], limit),
            Page {
                items: vec![3, 2],
                has_more: false
            }
        );
    }
}
