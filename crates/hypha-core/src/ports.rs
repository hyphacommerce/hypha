use thiserror::Error;
use uuid::Uuid;

pub trait IdGenerator {
    fn new_uuid(&self) -> Uuid;
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RepoError {
    #[error("storage backend error: {0}")]
    Backend(String),
}
