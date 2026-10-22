//! The port: what the server needs from Hypha, and nothing about how Hypha answers it.

use std::fmt;

use hypha_core::ErrorCode;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// An operation's stable ID, the OpenAPI `operationId` (`admin.products.update`).
///
/// It doubles as a tool name, so it keeps to the characters the MCP specification
/// recommends for those: 1 to 128 ASCII letters, digits, `.`, `_` and `-`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct OperationId(String);

impl OperationId {
    pub const MAX_LEN: usize = 128;

    pub fn new(raw: impl Into<String>) -> Result<Self, InvalidOperationId> {
        let raw = raw.into();
        let valid = !raw.is_empty()
            && raw.len() <= Self::MAX_LEN
            && raw
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
        if valid {
            Ok(Self(raw))
        } else {
            Err(InvalidOperationId(raw))
        }
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

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid operation ID {0:?}: expected 1 to 128 ASCII letters, digits, '.', '_' or '-'")]
pub struct InvalidOperationId(pub String);

impl ErrorCode for InvalidOperationId {
    fn code(&self) -> &'static str {
        "invalid_operation_id"
    }
}

/// Whether an operation only reads or may change the store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Access {
    Read,
    Write,
}

/// What this caller may do with an operation: `read` for reads; for writes, `propose`
/// (an apply becomes a proposal a person approves) or `apply`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Read,
    Propose,
    Apply,
}

/// How a call is to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallMode {
    /// Run it and roll it back: answers what it would return and record.
    Preview,
    /// Run it for real, or propose it when the caller's mode is `propose`.
    Apply,
    /// Store it as a proposal for a person to approve, whatever the caller's mode.
    Propose,
}

/// One operation the caller may call, as `find_operations` lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationSummary {
    pub id: OperationId,
    pub summary: String,
    /// The OpenAPI tags (`products`, `orders`), used in ranking.
    pub tags: Vec<String>,
    pub access: Access,
    pub mode: Mode,
}

/// The request body an operation takes.
#[derive(Debug, Clone, PartialEq)]
pub struct BodySchema {
    /// JSON Schema (2020-12, self-contained: no `$ref` to other documents).
    pub schema: Value,
    pub required: bool,
}

/// An error code an operation can answer, with its HTTP status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorDoc {
    pub code: String,
    pub status: u16,
    pub description: Option<String>,
}

/// Everything needed to call an operation and read its answer.
#[derive(Debug, Clone, PartialEq)]
pub struct OperationSchema {
    pub summary: OperationSummary,
    pub description: Option<String>,
    /// JSON Schema of an object holding the path parameters (`{"id": …}`).
    pub params: Value,
    /// JSON Schema of an object holding the query parameters.
    pub query: Value,
    pub body: Option<BodySchema>,
    /// JSON Schema of the success response's body, when it has one.
    pub output: Option<Value>,
    pub errors: Vec<ErrorDoc>,
    /// The scopes a caller needs.
    pub scopes: Vec<String>,
}

/// One call of an operation.
#[derive(Debug, Clone, PartialEq)]
pub struct OperationCall {
    pub operation: OperationId,
    pub params: Map<String, Value>,
    pub query: Map<String, Value>,
    pub body: Option<Value>,
    pub mode: CallMode,
}

/// An HTTP-shaped answer: the status and the JSON body the route sent, untouched.
///
/// A status outside 2xx is a tool error whose body is Hypha's `{code, error,
/// details}`, shown to the agent as is.
#[derive(Debug, Clone, PartialEq)]
pub struct OperationResponse {
    pub status: u16,
    /// `Value::Null` for a response without a body.
    pub body: Value,
}

impl OperationResponse {
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// A write stored for a person to approve instead of being applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposed {
    /// The proposal's ID (`prop_…`).
    pub proposal: String,
    /// Where a person reviews it in the admin.
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OperationResult {
    Response(OperationResponse),
    Proposed(Proposed),
}

/// A proposal's state, as `list_proposals` filters it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalStatus {
    Pending,
    Applied,
    Rejected,
    Expired,
    Failed,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProposalQuery {
    pub status: Option<ProposalStatus>,
    pub cursor: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum McpError {
    /// Not an operation, or not one this caller may see.
    #[error("there is no operation {0} available to you")]
    OperationNotFound(OperationId),
    /// Anything else. The client is told only "Internal error", so the implementation
    /// logs the cause before returning it.
    #[error("internal error: {0}")]
    Internal(String),
}

impl ErrorCode for McpError {
    fn code(&self) -> &'static str {
        match self {
            Self::OperationNotFound(_) => "operation_not_found",
            Self::Internal(_) => "internal_error",
        }
    }
}

/// What the MCP server reaches Hypha through, one implementation per request (it
/// knows the caller).
///
/// The server trusts it to decide visibility: an operation `visible` doesn't report
/// is never listed, and `describe` and `call` answer `OperationNotFound` for it.
#[allow(async_fn_in_trait)]
pub trait Operations {
    /// The operations this caller may call, with their mode.
    async fn visible(&self) -> Result<Vec<OperationSummary>, McpError>;

    async fn describe(&self, id: &OperationId) -> Result<OperationSchema, McpError>;

    async fn call(&self, call: OperationCall) -> Result<OperationResult, McpError>;

    /// The caller, its grant, the store, its currency and time zone.
    async fn whoami(&self) -> Result<OperationResponse, McpError>;

    /// One of the caller's own proposals.
    async fn proposal(&self, id: &str) -> Result<OperationResponse, McpError>;

    /// A page of the caller's own proposals, newest first.
    async fn proposals(&self, query: ProposalQuery) -> Result<OperationResponse, McpError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operation_ids_take_dots_underscores_and_hyphens() {
        assert!(OperationId::new("admin.orders.mark_paid").is_ok());
        assert!(OperationId::new("app.app_stripe.sync-now").is_ok());
    }

    #[test]
    fn operation_ids_refuse_empty_spaces_and_slashes() {
        for raw in ["", "admin orders", "admin/orders", "commandé"] {
            assert_eq!(
                OperationId::new(raw),
                Err(InvalidOperationId(raw.to_string()))
            );
        }
    }

    #[test]
    fn operation_ids_are_at_most_128_bytes() {
        assert!(OperationId::new("a".repeat(128)).is_ok());
        assert_eq!(
            OperationId::new("a".repeat(129)),
            Err(InvalidOperationId("a".repeat(129)))
        );
    }

    #[test]
    fn a_response_succeeds_only_in_2xx() {
        let response = |status| OperationResponse {
            status,
            body: Value::Null,
        };
        assert!(!response(199).is_success());
        assert!(response(200).is_success());
        assert!(response(299).is_success());
        assert!(!response(300).is_success());
    }
}
