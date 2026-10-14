//! Pages of a list (design note 0012): newest first, continued after the last item seen.
//!
//! IDs are UUIDv7, so ordering by ID is ordering by creation time; a page asks for the
//! items whose ID is below the last one it got (`after`). A cursor like this stays
//! correct while items are added, unlike an offset, and costs one indexed range scan.

use thiserror::Error;

use crate::ErrorCode;

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
