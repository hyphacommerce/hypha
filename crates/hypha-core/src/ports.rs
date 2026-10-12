use thiserror::Error;
use uuid::Uuid;

pub trait IdGenerator {
    fn new_uuid(&self) -> Uuid;
}

/// What a repository can report. Adapters (in-memory, Postgres, D1) must map their
/// failures onto these, so commands behave the same on every backend.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum RepoError {
    /// The backend failed (connection, corrupt data, ...). Never shown to clients.
    #[error("storage backend error: {0}")]
    Backend(String),
    /// An update was based on an old version: someone else changed the record since it
    /// was loaded (optimistic concurrency, see design note 0001). Safe to retry.
    #[error("the record was changed by someone else since it was loaded")]
    StaleVersion,
    /// A write would break a uniqueness rule. The key names the rule (e.g.
    /// `"product_handle"`); domain crates own the constants and map them to precise
    /// errors like `HandleTaken`.
    #[error("a record with the same {0} already exists")]
    Duplicate(&'static str),
}

impl crate::ErrorCode for RepoError {
    fn code(&self) -> &'static str {
        match self {
            RepoError::Backend(_) => "storage_error",
            RepoError::StaleVersion => "concurrent_modification",
            // Commands map the keys they know to specific codes; this is the fallback.
            RepoError::Duplicate(_) => "conflict",
        }
    }
}
