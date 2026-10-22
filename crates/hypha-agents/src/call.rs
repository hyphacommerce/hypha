//! The call log: one record per tool call an agent made, reads included, so staff can see
//! what an agent read and did. No bodies (they may hold personal data); what changed is in
//! the outbox, stamped with the agent.

use std::fmt;

use hypha_core::{AgentCallId, AgentId, Timestamp};

use crate::{CallError, OperationId};

/// What a call asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CallMode {
    Read,
    /// A write previewed and rolled back.
    Preview,
    /// A write kept as a proposal for a person.
    Propose,
    /// A write applied at once.
    Apply,
}

impl CallMode {
    pub const ALL: [CallMode; 4] = [
        CallMode::Read,
        CallMode::Preview,
        CallMode::Propose,
        CallMode::Apply,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            CallMode::Read => "read",
            CallMode::Preview => "preview",
            CallMode::Propose => "propose",
            CallMode::Apply => "apply",
        }
    }

    /// The mode `as_str` spells, or the text that isn't one.
    pub fn parse(s: &str) -> Result<Self, String> {
        Self::ALL
            .into_iter()
            .find(|m| m.as_str() == s)
            .ok_or_else(|| s.to_string())
    }
}

impl fmt::Display for CallMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The longest error code a call records.
pub const MAX_ERROR_CODE_LEN: usize = 64;

/// One call, as it ended. A record, not an aggregate: written once and never changed, so
/// it has no version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentCall {
    id: AgentCallId,
    agent: AgentId,
    operation: OperationId,
    mode: CallMode,
    status: u16,
    error_code: Option<String>,
    duration_ms: u32,
    created_at: Timestamp,
}

/// What a call is made of, as the host saw it end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewAgentCall {
    pub agent: AgentId,
    pub operation: OperationId,
    pub mode: CallMode,
    /// The HTTP status the operation answered with.
    pub status: u16,
    /// Hypha's error `code`, when it failed with one.
    pub error_code: Option<String>,
    pub duration_ms: u32,
    pub at: Timestamp,
}

fn check_error_code(code: &str) -> Result<(), CallError> {
    let snake = code.starts_with(|c: char| c.is_ascii_lowercase())
        && code
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
    if !snake || code.len() > MAX_ERROR_CODE_LEN {
        return Err(CallError::InvalidErrorCode(code.to_string()));
    }
    Ok(())
}

impl AgentCall {
    /// The status is an HTTP status (100 to 599); the error code snake_case.
    pub fn new(id: AgentCallId, new: NewAgentCall) -> Result<Self, CallError> {
        if !(100..=599).contains(&new.status) {
            return Err(CallError::InvalidStatus(new.status));
        }
        if let Some(code) = &new.error_code {
            check_error_code(code)?;
        }
        Ok(Self {
            id,
            agent: new.agent,
            operation: new.operation,
            mode: new.mode,
            status: new.status,
            error_code: new.error_code,
            duration_ms: new.duration_ms,
            created_at: new.at,
        })
    }

    pub fn id(&self) -> AgentCallId {
        self.id
    }

    pub fn agent(&self) -> AgentId {
        self.agent
    }

    pub fn operation(&self) -> &OperationId {
        &self.operation
    }

    pub fn mode(&self) -> CallMode {
        self.mode
    }

    pub fn status(&self) -> u16 {
        self.status
    }

    pub fn error_code(&self) -> Option<&str> {
        self.error_code.as_deref()
    }

    pub fn duration_ms(&self) -> u32 {
        self.duration_ms
    }

    pub fn created_at(&self) -> Timestamp {
        self.created_at
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;

    fn new(status: u16, error_code: Option<&str>) -> NewAgentCall {
        NewAgentCall {
            agent: AgentId::from_uuid(Uuid::from_u128(1)),
            operation: OperationId::new("admin.orders.list").unwrap(),
            mode: CallMode::Read,
            status,
            error_code: error_code.map(str::to_string),
            duration_ms: 12,
            at: Timestamp::from_unix_micros(3),
        }
    }

    fn id() -> AgentCallId {
        AgentCallId::from_uuid(Uuid::from_u128(2))
    }

    #[test]
    fn modes_parse_their_own_strings_only() {
        for mode in CallMode::ALL {
            assert_eq!(CallMode::parse(mode.as_str()), Ok(mode));
        }
        assert_eq!(CallMode::parse("write"), Err("write".to_string()));
    }

    #[test]
    fn the_status_is_an_http_status() {
        assert!(AgentCall::new(id(), new(100, None)).is_ok());
        assert!(AgentCall::new(id(), new(599, None)).is_ok());
        assert_eq!(
            AgentCall::new(id(), new(99, None)),
            Err(CallError::InvalidStatus(99))
        );
        assert_eq!(
            AgentCall::new(id(), new(600, None)),
            Err(CallError::InvalidStatus(600))
        );
    }

    #[test]
    fn the_error_code_is_snake_case_and_short() {
        let call = AgentCall::new(id(), new(409, Some("insufficient_stock"))).unwrap();
        assert_eq!(call.error_code(), Some("insufficient_stock"));
        let longest = "a".repeat(MAX_ERROR_CODE_LEN);
        assert!(AgentCall::new(id(), new(409, Some(&longest))).is_ok());
        for bad in [
            "",
            "Conflict",
            "not found",
            "_x",
            "9lives",
            &format!("{longest}a"),
        ] {
            assert_eq!(
                AgentCall::new(id(), new(409, Some(bad))),
                Err(CallError::InvalidErrorCode(bad.to_string())),
                "{bad:?}"
            );
        }
    }
}
