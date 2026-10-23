//! Preview tokens: what lets a write be applied exactly as it was previewed. A preview of an
//! admin write answers a token; sending the same write again with it hands the command the
//! IDs the preview was handed and commits only if what it does matches the preview (their
//! `digest`: the events recorded and the aggregates' rows written, with the versions found). The host issues and checks them; this is their record and
//! its port, like idempotency keys: a mechanism every command shares, owned by none.
//!
//! The token itself is shown once, in the preview; only its SHA-256 is stored.

use uuid::Uuid;

use crate::{Actor, RepoError, StoredEvent, Timestamp};

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
    /// The digest of what the preview did: the events it recorded and the rows it wrote.
    pub digest: String,
    pub expires_at: Timestamp,
    /// Set once a write applied it, in that write's transaction.
    pub used_at: Option<Timestamp>,
}

/// What a previewed or applied write did in the database, as its storage read it back
/// before ending it: what applying it exactly as previewed compares.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WriteRecord {
    /// The events it recorded and kept, in order.
    pub events: Vec<StoredEvent>,
    /// Each row of an aggregate it wrote, in order: with the version found, so a write
    /// over a record someone changed since the preview differs even when its events
    /// don't (an event names what changed, not what it was changed from).
    pub rows: Vec<WrittenRow>,
}

/// A row of an aggregate (a versioned record) a write wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrittenRow {
    /// What kind of record: the storage's name for it (`products`).
    pub kind: String,
    /// Its key, as text (`/` between the parts of a key of several).
    pub key: String,
    pub change: RowChange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowChange {
    Inserted,
    /// Updated from this version.
    Updated(i64),
    /// Deleted at this version.
    Deleted(i64),
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
