//! Where assets are used (design notes 0013, section 6, and 0014, E9). Usages are
//! recorded by whoever attaches an asset, in the same batch as the owner's write, so
//! "used by" is one lookup and an asset in use can't be deleted. Media never knows the
//! owners: their kind is a string, so spores can register their own.

use std::{fmt, str::FromStr};

use hypha_core::{AssetId, ProductId};

use crate::MediaError;

/// Longest owner kind, role or owner ID.
const MAX_LEN: usize = 255;

/// What uses an asset: a kind (`product`, a spore's `spore_lookbook`) and that owner's
/// ID as text. Ordered by kind, then ID, which is how usage pages are ordered.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UsageOwner {
    kind: String,
    id: String,
}

impl UsageOwner {
    pub const PRODUCT: &str = "product";

    /// `kind` is lowercase ASCII letters, digits and `_`, starting with a letter; `id`
    /// is any text without whitespace or `:`.
    pub fn new(kind: &str, id: &str) -> Result<Self, MediaError> {
        check_name(kind)?;
        if id.is_empty() || id.len() > MAX_LEN || id.chars().any(|c| c.is_whitespace() || c == ':')
        {
            return Err(MediaError::InvalidUsage(format!("owner ID {id:?}")));
        }
        Ok(Self {
            kind: kind.to_string(),
            id: id.to_string(),
        })
    }

    pub fn product(id: ProductId) -> Self {
        Self {
            kind: Self::PRODUCT.to_string(),
            id: id.to_string(),
        }
    }

    pub fn kind(&self) -> &str {
        &self.kind
    }

    pub fn id(&self) -> &str {
        &self.id
    }
}

/// `kind:id`, the form a usage page's cursor takes.
impl fmt::Display for UsageOwner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.kind, self.id)
    }
}

impl FromStr for UsageOwner {
    type Err = MediaError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (kind, id) = s
            .split_once(':')
            .ok_or_else(|| MediaError::InvalidUsage(format!("owner {s:?}")))?;
        Self::new(kind, id)
    }
}

/// How an owner uses an asset: products use `featured` (the first item of their media)
/// and `gallery` (the others); spores name their own roles by the same rule as kinds.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UsageRole(String);

impl UsageRole {
    pub const FEATURED: &str = "featured";
    pub const GALLERY: &str = "gallery";

    pub fn new(role: &str) -> Result<Self, MediaError> {
        check_name(role)?;
        Ok(Self(role.to_string()))
    }

    pub fn featured() -> Self {
        Self(Self::FEATURED.to_string())
    }

    pub fn gallery() -> Self {
        Self(Self::GALLERY.to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for UsageRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// One owner's use of one asset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetUsage {
    pub asset: AssetId,
    pub owner: UsageOwner,
    pub role: UsageRole,
}

/// Lowercase ASCII letters, digits and `_`, starting with a letter.
fn check_name(name: &str) -> Result<(), MediaError> {
    let mut chars = name.chars();
    let valid = name.len() <= MAX_LEN
        && chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if valid {
        Ok(())
    } else {
        Err(MediaError::InvalidUsage(format!("name {name:?}")))
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;

    #[test]
    fn a_product_owner_is_its_prefixed_id() {
        let id = ProductId::from_uuid(Uuid::from_u128(1));
        let owner = UsageOwner::product(id);
        assert_eq!(
            (owner.kind(), owner.id()),
            ("product", id.to_string().as_str())
        );
    }

    #[test]
    fn kinds_and_roles_are_lowercase_names() {
        assert!(UsageOwner::new("spore_lookbook", "lb_1").is_ok());
        assert!(UsageRole::new("hero2").is_ok());
        for bad in ["", "Product", "2d", "_x", "a-b", "a b"] {
            assert_eq!(
                UsageOwner::new(bad, "1"),
                Err(MediaError::InvalidUsage(format!("name {bad:?}"))),
                "{bad:?}"
            );
            assert!(UsageRole::new(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn an_owner_id_has_no_whitespace_or_colon() {
        for bad in ["", "a b", "a:b"] {
            assert_eq!(
                UsageOwner::new("product", bad),
                Err(MediaError::InvalidUsage(format!("owner ID {bad:?}")))
            );
        }
    }

    #[test]
    fn an_owner_round_trips_through_its_cursor_form() {
        let owner = UsageOwner::new("spore_lookbook", "lb_1").unwrap();
        assert_eq!(owner.to_string(), "spore_lookbook:lb_1");
        assert_eq!("spore_lookbook:lb_1".parse(), Ok(owner));
        assert!("no-colon".parse::<UsageOwner>().is_err());
    }

    #[test]
    fn owners_order_by_kind_then_id() {
        let owner = |kind, id| UsageOwner::new(kind, id).unwrap();
        let mut owners = vec![
            owner("spore_a", "1"),
            owner("product", "b"),
            owner("product", "a"),
        ];
        owners.sort();
        assert_eq!(
            owners,
            vec![
                owner("product", "a"),
                owner("product", "b"),
                owner("spore_a", "1")
            ]
        );
    }
}
