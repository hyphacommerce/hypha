//! Idempotency keys (design note 0007, O7): a caller that retries a request with the
//! same key gets the first request's result instead of running it twice.
//!
//! A command that takes a key first asks `earlier_result` whether this caller already
//! ran it. If not, it stages an `IdempotencyRecord` in the same `HasBatch` commit as its
//! own writes, so the key is recorded exactly when the effect is. Two requests with the
//! same key racing: one commit wins, the other fails on `keys::IDEMPOTENCY_KEY` and
//! replays the winner.

use thiserror::Error;
use uuid::Uuid;

use crate::{Actor, ErrorCode, RepoError, Timestamp};

/// Longest key, in characters (all ASCII, so also bytes).
pub const MAX_KEY_LEN: usize = 255;

/// Uniqueness rules an `IdempotencyRepo` enforces, reported as `RepoError::Duplicate`.
pub mod keys {
    /// One record per actor and key.
    pub const IDEMPOTENCY_KEY: &str = "idempotency_key";
}

/// A caller's key: 1 to `MAX_KEY_LEN` visible ASCII characters (no spaces), e.g. a
/// UUID. Keys are scoped to the actor, so two API clients can't collide.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IdempotencyKey(String);

impl IdempotencyKey {
    pub fn new(raw: &str) -> Result<Self, IdempotencyError> {
        let visible = raw.bytes().all(|b| b.is_ascii_graphic());
        if raw.is_empty() || raw.len() > MAX_KEY_LEN || !visible {
            return Err(IdempotencyError::InvalidKey);
        }
        Ok(Self(raw.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// What a caller sends with a command: the key as given (the command checks it) and a
/// fingerprint of the request, so a key reused for a different request is caught.
///
/// The adapter computes the fingerprint over its own input (the HTTP host: SHA-256 of
/// method, path, query and canonical JSON body), because Rust's `Hash` isn't stable
/// across compiler versions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdempotencyRequest {
    pub key: String,
    pub fingerprint: [u8; 32],
}

/// A request that ran: which actor sent which key for which operation, and the ID of
/// what it produced or changed (a cart, an order), which a replay reads again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdempotencyRecord {
    pub actor: Actor,
    pub key: IdempotencyKey,
    /// The command's name, e.g. `"create_cart"`: the same key for another command is a
    /// different request.
    pub operation: String,
    pub fingerprint: [u8; 32],
    pub result: Uuid,
    pub created_at: Timestamp,
}

impl IdempotencyRecord {
    pub fn new(
        actor: Actor,
        request: &IdempotencyRequest,
        operation: &str,
        result: Uuid,
        created_at: Timestamp,
    ) -> Result<Self, IdempotencyError> {
        Ok(Self {
            actor,
            key: IdempotencyKey::new(&request.key)?,
            operation: operation.to_string(),
            fingerprint: request.fingerprint,
            result,
            created_at,
        })
    }
}

/// A command's result, and whether it is a replay of an earlier request with the same
/// key (the host tells the caller with an `Idempotent-Replayed` header).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Idempotent<T> {
    pub value: T,
    pub replayed: bool,
}

impl<T> Idempotent<T> {
    pub fn fresh(value: T) -> Self {
        Self {
            value,
            replayed: false,
        }
    }

    pub fn replayed(value: T) -> Self {
        Self {
            value,
            replayed: true,
        }
    }
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum IdempotencyError {
    #[error("an idempotency key is 1 to {MAX_KEY_LEN} visible ASCII characters")]
    InvalidKey,
    #[error("this idempotency key was already used for a different request")]
    KeyReused,
}

impl ErrorCode for IdempotencyError {
    fn code(&self) -> &'static str {
        match self {
            IdempotencyError::InvalidKey => "invalid_idempotency_key",
            IdempotencyError::KeyReused => "idempotency_key_reused",
        }
    }
}

#[allow(async_fn_in_trait)]
pub trait IdempotencyRepo {
    async fn get(
        &self,
        actor: Actor,
        key: &IdempotencyKey,
    ) -> Result<Option<IdempotencyRecord>, RepoError>;
}

/// Whatever holds the `IdempotencyRepo`: a host's per-request ports, a test world.
pub trait HasIdempotency {
    type Idempotency: IdempotencyRepo;
    fn idempotency(&self) -> &Self::Idempotency;
}

/// Borrowed ports are ports too, so a host can lend them out per request.
impl<T: HasIdempotency + ?Sized> HasIdempotency for &T {
    type Idempotency = T::Idempotency;
    fn idempotency(&self) -> &Self::Idempotency {
        (**self).idempotency()
    }
}

/// Records a key in a `HasBatch` batch (records are only ever written that way, with
/// the effect they belong to). Enforces `keys::IDEMPOTENCY_KEY` on commit.
pub trait StageIdempotency {
    fn record_idempotency(&mut self, record: IdempotencyRecord);
}

/// What `actor` got the first time it sent `request` for `operation`: `Some(result)`
/// to replay, `None` if this is the first time. A key used for another operation or
/// with a different fingerprint is `KeyReused`; a malformed key is `InvalidKey`.
pub async fn earlier_result<R: IdempotencyRepo, E>(
    repo: &R,
    actor: Actor,
    operation: &str,
    request: &IdempotencyRequest,
) -> Result<Option<Uuid>, E>
where
    E: From<RepoError> + From<IdempotencyError>,
{
    let key = IdempotencyKey::new(&request.key)?;
    match repo.get(actor, &key).await? {
        None => Ok(None),
        Some(r) if r.operation == operation && r.fingerprint == request.fingerprint => {
            Ok(Some(r.result))
        }
        Some(_) => Err(IdempotencyError::KeyReused.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_is_visible_ascii() {
        assert!(IdempotencyKey::new("0f8b2c6e-7d1a-4e4b-9a55-6c1d2e3f4a5b").is_ok());
        assert_eq!(IdempotencyKey::new(""), Err(IdempotencyError::InvalidKey));
        assert_eq!(
            IdempotencyKey::new("two words"),
            Err(IdempotencyError::InvalidKey)
        );
        assert_eq!(
            IdempotencyKey::new("ključ"),
            Err(IdempotencyError::InvalidKey)
        );
    }

    #[test]
    fn a_key_is_at_most_255_characters() {
        assert!(IdempotencyKey::new(&"k".repeat(MAX_KEY_LEN)).is_ok());
        assert_eq!(
            IdempotencyKey::new(&"k".repeat(MAX_KEY_LEN + 1)),
            Err(IdempotencyError::InvalidKey)
        );
    }
}
