//! Preview tokens: what lets a write be applied exactly as it was previewed. A preview of an
//! admin write answers a token; sending the same write again with it hands the command the
//! IDs the preview was handed and commits only if the events it records match the
//! preview's (their `digest`). The host issues and checks them; this is their record and
//! its port, like idempotency keys: a mechanism every command shares, owned by none.
//!
//! The token itself is shown once, in the preview; only its SHA-256 is stored.

use uuid::Uuid;

use crate::{Actor, RepoError, Timestamp};

/// A preview's token as stored: who previewed what, the IDs it was handed and what its
/// events came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewToken {
    /// SHA-256 of the whole token (prefix included).
    pub hash: [u8; 32],
    /// Who previewed: only they may apply it.
    pub actor: Actor,
    /// The request previewed (method, path, query and body, as the host fingerprints
    /// them): only the same request may apply it.
    pub request: [u8; 32],
    /// The IDs the preview was handed, in order.
    pub ids: Vec<Uuid>,
    /// The digest of the events the preview recorded.
    pub digest: String,
    pub expires_at: Timestamp,
    /// Set once a write applied it, in that write's transaction.
    pub used_at: Option<Timestamp>,
}

#[allow(async_fn_in_trait)]
pub trait PreviewTokenRepo {
    /// Stores a new token. Hashes are random: a duplicate is the caller's bug, refused
    /// with `Duplicate(keys::PREVIEW_TOKEN)`.
    async fn insert(&self, token: &PreviewToken) -> Result<(), RepoError>;
    async fn get(&self, hash: &[u8; 32]) -> Result<Option<PreviewToken>, RepoError>;
    /// Marks the token used at `at`, unless it already was; whether this call did. A
    /// missing token is `false`. In a write's transaction, so a write that rolls back
    /// leaves it unused, and of two writes racing with one token only one commits it.
    async fn mark_used(&self, hash: &[u8; 32], at: Timestamp) -> Result<bool, RepoError>;
    /// Deletes up to `limit` tokens that expired before `before`; how many it deleted.
    async fn prune(&self, before: Timestamp, limit: u32) -> Result<u32, RepoError>;
}

/// Uniqueness rules the stores enforce, reported as `RepoError::Duplicate(key)`.
pub mod keys {
    pub const PREVIEW_TOKEN: &str = "preview_token";
}

pub trait HasPreviewTokens {
    type PreviewTokens: PreviewTokenRepo;
    fn preview_tokens(&self) -> &Self::PreviewTokens;
}

impl<T: HasPreviewTokens + ?Sized> HasPreviewTokens for &T {
    type PreviewTokens = T::PreviewTokens;
    fn preview_tokens(&self) -> &Self::PreviewTokens {
        (**self).preview_tokens()
    }
}
