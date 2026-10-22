//! Operation IDs: the names agents call admin operations by (`admin.products.update`), the
//! same as the OpenAPI documents' `operationId`s. Renaming one breaks agents' prompts, like
//! renaming an error code.

use std::fmt;

use hypha_core::ErrorCode;
use thiserror::Error;

/// Dot-separated segments, at least two: the first a lowercase word (`admin`, `app`), the
/// rest lowercase letters, digits, `_` and `-` (an app action's name may have dashes:
/// `app.app_reviews.import-csv`). At most `MAX_LEN` bytes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OperationId(String);

#[derive(Debug, Clone, Error, PartialEq, Eq)]
#[error("{0:?} is not an operation ID like \"admin.products.update\"")]
pub struct OperationError(pub String);

impl ErrorCode for OperationError {
    fn code(&self) -> &'static str {
        "invalid_operation_id"
    }
}

impl OperationId {
    pub const MAX_LEN: usize = 200;

    pub fn new(raw: &str) -> Result<Self, OperationError> {
        let invalid = || OperationError(raw.to_string());
        if raw.is_empty() || raw.len() > Self::MAX_LEN {
            return Err(invalid());
        }
        let mut segments = raw.split('.');
        let first = segments.next().ok_or_else(invalid)?;
        let first_ok = first.starts_with(|c: char| c.is_ascii_lowercase())
            && first
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
        if !first_ok {
            return Err(invalid());
        }
        let mut rest = 0;
        for segment in segments {
            let ok = segment.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
                && segment.bytes().all(|b| {
                    b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-'
                });
            if !ok {
                return Err(invalid());
            }
            rest += 1;
        }
        if rest == 0 {
            return Err(invalid());
        }
        Ok(Self(raw.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for OperationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takes_the_route_tables_ids() {
        for id in [
            "admin.products.update",
            "admin.products.update_variant",
            "admin.inventory.fetch_product_inventory",
            "app.app_reviews.import-csv",
            "spore_stripe.webhook",
        ] {
            assert_eq!(
                OperationId::new(id).map(|o| o.to_string()),
                Ok(id.to_string())
            );
        }
    }

    #[test]
    fn refuses_other_shapes() {
        for id in [
            "",
            "admin",
            "admin.",
            ".products",
            "admin..update",
            "Admin.products",
            "admin.Products",
            "admin.products update",
            "1admin.products",
            "admin.-products",
            "admin-x.products",
        ] {
            assert_eq!(
                OperationId::new(id),
                Err(OperationError(id.to_string())),
                "{id:?}"
            );
        }
    }

    #[test]
    fn is_at_most_max_len_bytes() {
        let at = format!("admin.{}", "a".repeat(OperationId::MAX_LEN - 6));
        assert!(OperationId::new(&at).is_ok());
        let over = format!("{at}a");
        assert_eq!(OperationId::new(&over), Err(OperationError(over.clone())));
    }
}
