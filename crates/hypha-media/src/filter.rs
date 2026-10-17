//! What the library list is narrowed to and how it is ordered (design note 0024, §2).

use hypha_core::{Direction, SortKey, optional_text};

use crate::{Asset, AssetKind, MediaError};

/// The library's filters, checked. Only ready assets are ever listed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AssetFilter {
    kind: Option<AssetKind>,
    q: Option<String>,
}

impl AssetFilter {
    /// `q`, text the file name, alt text or title contains (ignoring case), follows the
    /// shared text rule; blank is no filter.
    pub fn new(kind: Option<AssetKind>, q: Option<String>) -> Result<Self, MediaError> {
        Ok(Self {
            kind,
            q: optional_text("q", q)?,
        })
    }

    pub fn kind(&self) -> Option<AssetKind> {
        self.kind
    }

    pub fn q(&self) -> Option<&str> {
        self.q.as_deref()
    }

    /// Whether a ready `asset` is in the list. Adapters that filter in storage must
    /// agree (the repository contract checks).
    pub fn matches(&self, asset: &Asset) -> bool {
        self.kind.is_none_or(|k| asset.kind() == k)
            && self.q.as_ref().is_none_or(|q| {
                let q = q.to_lowercase();
                let has = |text: &str| text.to_lowercase().contains(&q);
                has(asset.filename())
                    || asset.alt().is_some_and(has)
                    || asset.title().is_some_and(has)
            })
    }
}

/// How the library is ordered (design note 0024, AU1); names compare as `SortKey::text`
/// does, ties by ID in the same direction.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AssetSort {
    #[default]
    Newest,
    Oldest,
    /// By file name.
    NameAsc,
    NameDesc,
}

impl AssetSort {
    pub fn direction(self) -> Direction {
        match self {
            AssetSort::Oldest | AssetSort::NameAsc => Direction::Ascending,
            AssetSort::Newest | AssetSort::NameDesc => Direction::Descending,
        }
    }

    pub fn key(self, asset: &Asset) -> SortKey {
        match self {
            AssetSort::Newest | AssetSort::Oldest => SortKey::Id,
            AssetSort::NameAsc | AssetSort::NameDesc => SortKey::text(asset.filename()),
        }
    }

    /// Whether `key` is the kind this sort uses: a cursor made for another sort doesn't.
    pub fn fits(self, key: &SortKey) -> bool {
        matches!(
            (self, key),
            (AssetSort::Newest | AssetSort::Oldest, SortKey::Id)
                | (AssetSort::NameAsc | AssetSort::NameDesc, SortKey::Text(_))
        )
    }
}

#[cfg(test)]
mod tests {
    use hypha_core::TextError;

    use super::*;

    #[test]
    fn text_follows_the_text_rule() {
        assert_eq!(
            AssetFilter::new(None, Some("x".repeat(256))),
            Err(MediaError::Text(TextError::TooLong("q")))
        );
        assert_eq!(
            AssetFilter::new(None, Some("  ".to_string())),
            Ok(AssetFilter::default())
        );
    }

    #[test]
    fn each_sort_fits_only_its_own_key() {
        assert!(AssetSort::Oldest.fits(&SortKey::Id));
        assert!(AssetSort::NameDesc.fits(&SortKey::text("a.png")));
        assert!(!AssetSort::NameAsc.fits(&SortKey::Id));
    }
}
