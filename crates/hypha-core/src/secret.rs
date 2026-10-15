//! Encryption for secrets that must be read back (design note 0008, A5): TOTP seeds,
//! shipping callback secrets (0015). Passwords and tokens are hashed instead. This is
//! the port; `hypha_auth::XChaChaBox` is the cipher hosts use.

use std::fmt;

use thiserror::Error;

/// Encrypted bytes as stored: a format version, the nonce, then the ciphertext with its
/// authentication tag. The version byte lets a later key or algorithm coexist with
/// what's already stored.
#[derive(Clone, PartialEq, Eq)]
pub struct Sealed(Vec<u8>);

impl Sealed {
    /// For storage adapters reading it back.
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for Sealed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Sealed({} bytes)", self.0.len())
    }
}

/// `open` failed: the wrong key, tampered or truncated data, or data sealed for another
/// record (`context`). Reported as a server error.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
#[error("cannot open sealed data: {0}")]
pub struct SealError(pub &'static str);

/// Seals and opens secrets with a key only the host knows (`HYPHA_SECRET_KEY`).
///
/// `context` is bound into the ciphertext (authenticated, not encrypted), e.g. the
/// identity's ID, so a sealed value copied onto another record fails to open.
pub trait SecretBox {
    /// `nonce` must never repeat under one key; it comes from `hypha_core::Random`.
    fn seal(&self, plaintext: &[u8], context: &[u8], nonce: [u8; 24]) -> Sealed;
    fn open(&self, sealed: &Sealed, context: &[u8]) -> Result<Vec<u8>, SealError>;
}
