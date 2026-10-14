//! Passwords and the hashing port (design note 0008, section 2).

use std::fmt;

use argon2::{Algorithm, Argon2, Params, PasswordHasher as _, PasswordVerifier as _, Version};
use hypha_core::ErrorCode;
use thiserror::Error;

/// A password someone wants to set: 8 to 128 characters, taken as typed (no trimming,
/// no normalization). Only for *setting* a password; a login attempt is checked with
/// `Password::attempt`, so tightening these rules never locks anyone out.
///
/// `Debug` prints `Password(***)`: a password must never reach a log.
#[derive(Clone, PartialEq, Eq)]
pub struct Password(String);

#[derive(Debug, Clone, Copy, Error, PartialEq, Eq)]
pub enum PasswordError {
    #[error("the password needs at least {min} characters")]
    TooShort { min: usize },
    #[error("the password can be at most {max} characters")]
    TooLong { max: usize },
}

impl ErrorCode for PasswordError {
    fn code(&self) -> &'static str {
        match self {
            PasswordError::TooShort { .. } => "password_too_short",
            PasswordError::TooLong { .. } => "password_too_long",
        }
    }
}

impl Password {
    pub const MIN_CHARS: usize = 8;
    pub const MAX_CHARS: usize = 128;
    /// Longest login attempt worth hashing. Longer ones can't match any password set
    /// under the rules above, and hashing megabytes would be a cheap way to burn CPU.
    const MAX_ATTEMPT_BYTES: usize = 1024;

    pub fn new(password: &str) -> Result<Self, PasswordError> {
        let chars = password.chars().count();
        if chars < Self::MIN_CHARS {
            return Err(PasswordError::TooShort {
                min: Self::MIN_CHARS,
            });
        }
        if chars > Self::MAX_CHARS {
            return Err(PasswordError::TooLong {
                max: Self::MAX_CHARS,
            });
        }
        Ok(Self(password.to_string()))
    }

    /// A password as typed at login, with no rules applied. `None` if it's too long to
    /// be anyone's password; the caller treats that like a wrong one.
    pub fn attempt(password: &str) -> Option<Self> {
        (password.len() <= Self::MAX_ATTEMPT_BYTES).then(|| Self(password.to_string()))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Password {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Password(***)")
    }
}

/// A stored password hash in PHC string format (`$argon2id$v=19$m=…`). The format names
/// the algorithm and its parameters, so a hash made with older parameters still
/// verifies after they change.
#[derive(Clone, PartialEq, Eq)]
pub struct PasswordHash(String);

impl PasswordHash {
    /// For storage adapters and hashers. Not checked here: a hash that doesn't parse
    /// simply never verifies.
    pub fn from_phc(phc: String) -> Self {
        Self(phc)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for PasswordHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PasswordHash(..)")
    }
}

/// The hashing failed for a reason other than a wrong password (bad parameters, a
/// corrupt stored hash). Reported as a server error; the text is for logs.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
#[error("password hashing failed: {0}")]
pub struct HashError(pub String);

/// Slow password hashing, as a port (design note 0008, section 2). It's async so a host
/// can move the work off its request threads (the axum host uses a blocking thread),
/// and a Workers host could later hand it elsewhere.
#[allow(async_fn_in_trait)]
pub trait PasswordHasher {
    /// `salt` comes from `hypha_core::Random`, 16 bytes.
    async fn hash_password(
        &self,
        password: &Password,
        salt: [u8; 16],
    ) -> Result<PasswordHash, HashError>;

    /// Whether `password` matches `hash`. A hash this hasher can't read is `Ok(false)`.
    async fn verify_password(
        &self,
        password: &Password,
        hash: &PasswordHash,
    ) -> Result<bool, HashError>;
}

/// Argon2id, the default `PasswordHasher`. Pure Rust, so it runs on every host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Argon2id {
    memory_kib: u32,
    passes: u32,
    lanes: u32,
}

impl Argon2id {
    /// OWASP's recommended minimum for Argon2id: 19 MiB, 2 passes, 1 lane. A hash takes
    /// tens of milliseconds natively.
    pub const OWASP: Argon2id = Argon2id {
        memory_kib: 19 * 1024,
        passes: 2,
        lanes: 1,
    };

    /// Other parameters, e.g. cheap ones for tests. Invalid ones fail on first use.
    pub const fn new(memory_kib: u32, passes: u32, lanes: u32) -> Self {
        Self {
            memory_kib,
            passes,
            lanes,
        }
    }

    fn argon2(&self) -> Result<Argon2<'static>, HashError> {
        let params = Params::new(self.memory_kib, self.passes, self.lanes, None)
            .map_err(|e| HashError(e.to_string()))?;
        Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, params))
    }

    /// The blocking work behind `hash_password`, for hosts that run it on a thread of
    /// their own.
    pub fn hash_now(&self, password: &Password, salt: [u8; 16]) -> Result<PasswordHash, HashError> {
        let hash = self
            .argon2()?
            .hash_password_with_salt(password.expose().as_bytes(), &salt)
            .map_err(|e| HashError(e.to_string()))?;
        Ok(PasswordHash(hash.to_string()))
    }

    /// The blocking work behind `verify_password`. Verifies with the parameters stored
    /// in the hash, not `self`'s.
    pub fn verify_now(&self, password: &Password, hash: &PasswordHash) -> Result<bool, HashError> {
        Ok(self
            .argon2()?
            .verify_password(password.expose().as_bytes(), hash.as_str())
            .is_ok())
    }
}

impl PasswordHasher for Argon2id {
    async fn hash_password(
        &self,
        password: &Password,
        salt: [u8; 16],
    ) -> Result<PasswordHash, HashError> {
        self.hash_now(password, salt)
    }

    async fn verify_password(
        &self,
        password: &Password,
        hash: &PasswordHash,
    ) -> Result<bool, HashError> {
        self.verify_now(password, hash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHEAP: Argon2id = Argon2id::new(64, 1, 1);

    #[test]
    fn length_counts_characters_and_has_both_bounds() {
        assert_eq!(
            Password::new("1234567"),
            Err(PasswordError::TooShort { min: 8 })
        );
        assert!(Password::new("12345678").is_ok());
        // 8 characters, 16 bytes.
        assert!(Password::new("čćčćčćčć").is_ok());
        assert!(Password::new(&"x".repeat(128)).is_ok());
        assert_eq!(
            Password::new(&"x".repeat(129)),
            Err(PasswordError::TooLong { max: 128 })
        );
    }

    #[test]
    fn spaces_are_part_of_the_password() {
        assert_eq!(
            Password::new(" secret pass ").unwrap().expose(),
            " secret pass "
        );
    }

    #[test]
    fn an_attempt_has_no_rules_but_a_size_cap() {
        assert!(Password::attempt("short").is_some());
        assert!(Password::attempt(&"x".repeat(1024)).is_some());
        assert!(Password::attempt(&"x".repeat(1025)).is_none());
    }

    #[test]
    fn debug_hides_the_password() {
        assert_eq!(
            format!("{:?}", Password::new("hunter2hunter2").unwrap()),
            "Password(***)"
        );
    }

    #[test]
    fn a_hash_verifies_its_password_only() {
        let password = Password::new("correct horse").unwrap();
        let hash = CHEAP.hash_now(&password, [7; 16]).unwrap();
        assert!(hash.as_str().starts_with("$argon2id$v=19$m=64,t=1,p=1$"));
        assert_eq!(CHEAP.verify_now(&password, &hash), Ok(true));
        let wrong = Password::attempt("correct horsf").unwrap();
        assert_eq!(CHEAP.verify_now(&wrong, &hash), Ok(false));
    }

    #[test]
    fn verifying_uses_the_parameters_in_the_hash() {
        let password = Password::new("correct horse").unwrap();
        let old = Argon2id::new(32, 1, 1)
            .hash_now(&password, [7; 16])
            .unwrap();
        assert_eq!(CHEAP.verify_now(&password, &old), Ok(true));
    }

    #[test]
    fn an_unreadable_hash_never_verifies() {
        let password = Password::new("correct horse").unwrap();
        let junk = PasswordHash::from_phc("not a hash".to_string());
        assert_eq!(CHEAP.verify_now(&password, &junk), Ok(false));
    }
}
