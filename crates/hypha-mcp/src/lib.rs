//! The Model Context Protocol server for Hypha's admin: JSON-RPC envelopes, the
//! protocol versions it speaks, and the tools that reach every admin operation.
//!
//! It knows nothing of routes, storage, HTTP frameworks or grants. Hypha reaches it
//! through one port, [`Operations`], implemented per request by the host's HTTP layer:
//! what a caller may do is whatever `visible` reports.
//!
//! The transport hands [`Server::handle`] one POST's body and the headers MCP mirrors
//! (`MCP-Protocol-Version`, `Mcp-Method`, `Mcp-Name`) and answers with the status and
//! JSON body to send:
//!
//! ```
//! use hypha_mcp::{Headers, McpRequest, Server, ServerConfig};
//! # use hypha_mcp::*;
//! # struct NoOps;
//! # impl Operations for NoOps {
//! #     async fn visible(&self) -> Result<Vec<OperationSummary>, McpError> { Ok(vec![]) }
//! #     async fn describe(&self, id: &OperationId) -> Result<OperationSchema, McpError> {
//! #         Err(McpError::OperationNotFound(id.clone()))
//! #     }
//! #     async fn call(&self, call: OperationCall) -> Result<OperationResult, McpError> {
//! #         Err(McpError::OperationNotFound(call.operation))
//! #     }
//! #     async fn whoami(&self) -> Result<OperationResponse, McpError> {
//! #         Ok(OperationResponse { status: 200, body: serde_json::json!({}) })
//! #     }
//! #     async fn proposal(&self, _: &str) -> Result<OperationResponse, McpError> {
//! #         Ok(OperationResponse { status: 404, body: serde_json::json!({}) })
//! #     }
//! #     async fn proposals(&self, _: ProposalQuery) -> Result<OperationResponse, McpError> {
//! #         Ok(OperationResponse { status: 200, body: serde_json::json!({}) })
//! #     }
//! # }
//! let server = Server::new(ServerConfig::new("hypha", "0.1.0")).expect("valid config");
//! let body = br#"{"jsonrpc":"2.0","id":1,"method":"initialize",
//!     "params":{"protocolVersion":"2025-11-25","capabilities":{},
//!     "clientInfo":{"name":"client","version":"1"}}}"#;
//! let request = McpRequest { body, headers: Headers::default() };
//! let response = pollster::block_on(server.handle(&NoOps, request));
//! assert_eq!(response.status, 200);
//! ```

pub mod jsonrpc;
pub mod ops;
pub mod protocol;
pub mod server;
pub mod tools;

pub use ops::{
    Access, BodySchema, CallMode, ErrorDoc, McpError, Mode, OperationCall, OperationId,
    OperationIdError, OperationResponse, OperationResult, OperationSchema, OperationSummary,
    Operations, ProposalQuery, ProposalStatus, Proposed,
};
pub use protocol::{
    Headers, LEGACY_VERSIONS, METHOD_HEADER, MODERN_VERSIONS, NAME_HEADER, PROTOCOL_VERSION_HEADER,
};
pub use server::{
    ConfigError, LIST_TTL_MS, MAX_TOOL_NAME_LEN, McpRequest, McpResponse, Server, ServerConfig,
};
pub use tools::FIXED_TOOLS;
