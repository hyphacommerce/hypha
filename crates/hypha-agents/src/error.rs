use hypha_core::{AgentId, CallbackUrlError, ErrorCode, IdempotencyError, TextError};
use thiserror::Error;

use crate::{GrantError, OperationIdError, ProposalStatus};

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum AgentError {
    #[error(transparent)]
    Text(#[from] TextError),
    #[error(transparent)]
    Grant(#[from] GrantError),
    /// A client ID is the URL of its metadata document.
    #[error(transparent)]
    ClientId(#[from] CallbackUrlError),
    /// Revoking is for good: a revoked agent changes no more.
    #[error("agent {0} is revoked")]
    Revoked(AgentId),
}

impl ErrorCode for AgentError {
    fn code(&self) -> &'static str {
        match self {
            AgentError::Text(e) => e.code(),
            AgentError::Grant(e) => e.code(),
            AgentError::ClientId(e) => e.code(),
            AgentError::Revoked(_) => "agent_revoked",
        }
    }
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ProposalError {
    #[error(transparent)]
    Operation(#[from] OperationIdError),
    #[error(transparent)]
    Idempotency(#[from] IdempotencyError),
    /// Pending is the only state a proposal leaves.
    #[error("the proposal is already {0}")]
    Decided(ProposalStatus),
    #[error("the proposal expired")]
    Expired,
    /// Expiring one before its time (a job's bug).
    #[error("the proposal hasn't expired yet")]
    NotDue,
    #[error("the {what} is over {max} bytes")]
    TooLarge { what: &'static str, max: usize },
    /// The request's path parameters are text.
    #[error("path parameter {0:?} is not a string")]
    InvalidParam(String),
    #[error("{0:?} is not a preview digest")]
    InvalidDigest(String),
    #[error("{0} is not an HTTP status")]
    InvalidStatus(u16),
    #[error("a rejection note is at most {max} characters")]
    NoteTooLong { max: usize },
    /// Stored decided without a decision (or the reverse): only `restore` returns it.
    #[error("a stored proposal's decision doesn't match its status")]
    Inconsistent,
}

impl ErrorCode for ProposalError {
    fn code(&self) -> &'static str {
        match self {
            ProposalError::Operation(e) => e.code(),
            ProposalError::Idempotency(e) => e.code(),
            ProposalError::Decided(_) => "proposal_decided",
            ProposalError::Expired => "proposal_expired",
            ProposalError::NotDue => "internal_error",
            ProposalError::TooLarge { .. } => "proposal_too_large",
            ProposalError::InvalidParam(_) => "invalid_proposal_request",
            ProposalError::InvalidDigest(_) => "invalid_digest",
            ProposalError::InvalidStatus(_) => "invalid_status",
            ProposalError::NoteTooLong { .. } => "field_too_long",
            ProposalError::Inconsistent => "internal_error",
        }
    }
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum CallError {
    #[error(transparent)]
    Operation(#[from] OperationIdError),
    #[error("{0} is not an HTTP status")]
    InvalidStatus(u16),
    /// Error codes are snake_case, like every code Hypha answers with.
    #[error("{0:?} is not an error code")]
    InvalidErrorCode(String),
}

impl ErrorCode for CallError {
    fn code(&self) -> &'static str {
        match self {
            CallError::Operation(e) => e.code(),
            CallError::InvalidStatus(_) => "invalid_status",
            CallError::InvalidErrorCode(_) => "invalid_error_code",
        }
    }
}
