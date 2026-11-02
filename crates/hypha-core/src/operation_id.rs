//! Operation IDs: the names agents call admin operations by (`admin.products.update`), the
//! OpenAPI documents' `operationId`s. Renaming one breaks agents' prompts, like renaming an
//! error code. Agents' grants name them (`hypha-agents`) and MCP lists them as tools
//! (`hypha-mcp`), so the one type lives here.

use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::ErrorCode;

/// Dot-separated segments, at least two: the first a lowercase word (`admin`, `app`), the
/// rest lowercase letters, digits, `_` and `-` (an app action's name may have dashes:
/// `app.app_reviews.import-csv`), each starting with a letter or digit. Never `__` nor an
/// `_` ending a segment: a tool name is the ID with each `.` as `__` (`tool_name`), which
/// reads back, and never names two IDs alike, only while the ID holds neither. At most
/// `MAX_LEN` bytes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct OperationId(String);

#[derive(Debug, Clone, Error, PartialEq, Eq)]
#[error("{0:?} is not an operation ID like \"admin.products.update\"")]
pub struct OperationIdError(pub String);

impl ErrorCode for OperationIdError {
    fn code(&self) -> &'static str {
        "invalid_operation_id"
    }
}

fn first_segment(s: &str) -> bool {
    s.starts_with(|c: char| c.is_ascii_lowercase())
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn later_segment(s: &str) -> bool {
    s.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

impl OperationId {
    pub const MAX_LEN: usize = 128;

    pub fn new(raw: impl Into<String>) -> Result<Self, OperationIdError> {
        let raw = raw.into();
        let mut segments = raw.split('.');
        let valid = raw.len() <= Self::MAX_LEN
            && !raw.contains("__")
            && !raw.contains("_.")
            && !raw.ends_with('_')
            && segments.next().is_some_and(first_segment)
            && raw.contains('.')
            && segments.all(later_segment);
        if valid {
            Ok(Self(raw))
        } else {
            Err(OperationIdError(raw))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The operation's name as an MCP tool. Model APIs (Anthropic's, OpenAI's) take only
    /// `[a-zA-Z0-9_-]` in tool names, and MCP clients pass names through to them, so a dot
    /// can't stay: each becomes `__`.
    pub fn tool_name(&self) -> String {
        self.0.replace('.', "__")
    }

    /// The operation a tool's name stands for (`tool_name`'s inverse).
    pub fn from_tool_name(name: &str) -> Result<Self, OperationIdError> {
        // A dot is never in a tool name, so the dotted ID doesn't pass for one.
        if name.contains('.') {
            return Err(OperationIdError(name.to_string()));
        }
        Self::new(name.replace("__", "."))
    }
}

impl fmt::Display for OperationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for OperationId {
    type Error = OperationIdError;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        Self::new(raw)
    }
}

impl From<OperationId> for String {
    fn from(id: OperationId) -> String {
        id.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refused(raw: &str) {
        assert_eq!(
            OperationId::new(raw),
            Err(OperationIdError(raw.to_string())),
            "{raw:?}"
        );
    }

    #[test]
    fn takes_route_ids_and_app_actions() {
        for id in [
            "admin.products.update",
            "admin.orders.mark_paid",
            "admin.inventory.fetch_product_inventory",
            "app.app_reviews.import-csv",
            "app.app_stripe.sync-now",
            "spore_stripe.webhook",
            "admin.v2.list",
        ] {
            assert_eq!(
                OperationId::new(id).map(|o| o.to_string()),
                Ok(id.to_string())
            );
        }
    }

    #[test]
    fn refuses_other_shapes() {
        for raw in [
            "",
            "admin",
            "call",
            "admin.",
            ".products",
            "admin..update",
            "Admin.products",
            "admin.Products",
            "admin.products update",
            "admin/orders",
            "commandé.list",
            "1admin.products",
            "admin.-products",
            "admin-x.products",
        ] {
            refused(raw);
        }
    }

    #[test]
    fn refuses_what_would_make_tool_names_ambiguous() {
        // "a._b" and "a_.b" would both be the tool "a___b"; "a__b" would read back as "a.b".
        for raw in [
            "admin.orders__list",
            "admin._x",
            "admin_.x",
            "admin.x_",
            "admin.x_.y",
        ] {
            refused(raw);
        }
    }

    #[test]
    fn is_at_most_max_len_bytes() {
        let at = format!("admin.{}", "a".repeat(OperationId::MAX_LEN - 6));
        assert!(OperationId::new(at.clone()).is_ok());
        refused(&format!("{at}a"));
    }

    #[test]
    fn a_tool_name_has_no_dots_and_reads_back() {
        let id = OperationId::new("app.app_x.import-csv").unwrap();
        assert_eq!(id.tool_name(), "app__app_x__import-csv");
        assert_eq!(OperationId::from_tool_name(&id.tool_name()), Ok(id));
        assert_eq!(
            OperationId::from_tool_name("admin.orders.list"),
            Err(OperationIdError("admin.orders.list".to_string()))
        );
        assert!(OperationId::from_tool_name("a___b").is_err());
    }

    #[test]
    fn json_is_the_string_and_checked() {
        let id = OperationId::new("admin.orders.list").unwrap();
        assert_eq!(serde_json::to_string(&id).unwrap(), "\"admin.orders.list\"");
        assert_eq!(
            serde_json::from_str::<OperationId>("\"admin.orders.list\"").unwrap(),
            id
        );
        assert!(serde_json::from_str::<OperationId>("\"admin\"").is_err());
    }

    /// Every `operationId` in the committed OpenAPI documents parses, and its tool name
    /// reads back to it.
    #[test]
    fn every_documented_operation_id_parses() {
        fn collect(value: &serde_json::Value, found: &mut Vec<String>) {
            match value {
                serde_json::Value::Object(map) => {
                    for (key, v) in map {
                        match (key.as_str(), v) {
                            ("operationId", serde_json::Value::String(id)) => {
                                found.push(id.clone());
                            }
                            _ => collect(v, found),
                        }
                    }
                }
                serde_json::Value::Array(items) => items.iter().for_each(|v| collect(v, found)),
                _ => {}
            }
        }
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../openapi");
        let mut found = Vec::new();
        for name in ["admin.json", "storefront.json", "extensions.json"] {
            let text = std::fs::read_to_string(dir.join(name)).unwrap();
            collect(&serde_json::from_str(&text).unwrap(), &mut found);
        }
        assert!(found.len() > 200, "{} operation IDs", found.len());
        for raw in found {
            let id = OperationId::new(raw.as_str()).unwrap_or_else(|e| panic!("{e}"));
            assert_eq!(OperationId::from_tool_name(&id.tool_name()), Ok(id));
        }
    }
}
