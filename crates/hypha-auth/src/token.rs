//! Secret tokens: sessions, API keys, invites, password resets (design note 0008, A9).
//!
//! A token is a prefix that says what it is for, then 32 random bytes as hex. Only its
//! SHA-256 (`TokenHash`) is stored: the bytes are random and long enough that a fast
//! hash is safe, unlike a password, and a database leak then reveals no usable token.

use std::fmt;

use hypha_core::Random;
use sha2::{Digest, Sha256};

/// What a token is for. The prefix lets the host pick the lookup and lets secret
/// scanners (GitHub's, for instance) recognize a leaked token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenKind {
    Session,
    SecretKey,
    PublishableKey,
    Invite,
    PasswordReset,
}

impl TokenKind {
    const ALL: [TokenKind; 5] = [
        TokenKind::Session,
        TokenKind::SecretKey,
        TokenKind::PublishableKey,
        TokenKind::Invite,
        TokenKind::PasswordReset,
    ];

    pub const fn prefix(self) -> &'static str {
        match self {
            TokenKind::Session => "hypha_sess_",
            TokenKind::SecretKey => "hypha_sk_",
            TokenKind::PublishableKey => "hypha_pk_",
            TokenKind::Invite => "hypha_inv_",
            TokenKind::PasswordReset => "hypha_rst_",
        }
    }

    /// The kind a token claims to be by its prefix and shape, `None` for anything else.
    /// Says nothing about whether the token exists.
    pub fn of(token: &str) -> Option<TokenKind> {
        Self::ALL.into_iter().find(|kind| {
            token.strip_prefix(kind.prefix()).is_some_and(|rest| {
                rest.len() == TOKEN_BYTES * 2
                    && rest.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
            })
        })
    }
}

/// Random bytes in a token: 256 bits, far beyond guessing.
const TOKEN_BYTES: usize = 32;

/// SHA-256 of a whole token (prefix included): what storage keeps and looks up by.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct TokenHash([u8; 32]);

impl TokenHash {
    pub fn of(token: &str) -> Self {
        Self(Sha256::digest(token.as_bytes()).into())
    }

    /// For storage adapters reading a stored hash back.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for TokenHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Enough to tell two apart in a test failure; the whole hash is never useful.
        write!(f, "TokenHash({}…)", hex(&self.0[..4]))
    }
}

/// A freshly generated token. The caller shows it to its owner once (a login response,
/// an email); only `hash` is stored.
///
/// `Debug` hides the secret part, so a token that ends up in a log line or a test
/// failure doesn't leak. That's why this isn't a plain `String`.
#[derive(Clone, PartialEq, Eq)]
pub struct NewToken {
    token: String,
    hash: TokenHash,
}

impl NewToken {
    pub fn generate(kind: TokenKind, random: &impl Random) -> Self {
        let mut bytes = [0u8; TOKEN_BYTES];
        random.fill(&mut bytes);
        let token = format!("{}{}", kind.prefix(), hex(&bytes));
        let hash = TokenHash::of(&token);
        Self { token, hash }
    }

    /// The secret itself, for the one response or message that delivers it.
    pub fn expose(&self) -> &str {
        &self.token
    }

    pub fn hash(&self) -> TokenHash {
        self.hash
    }

    /// The last four characters, for telling keys apart in a list (`hypha_sk_…3f9a`).
    pub fn last_four(&self) -> &str {
        &self.token[self.token.len() - 4..]
    }
}

impl fmt::Debug for NewToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NewToken({:?})", self.hash)
    }
}

/// Lowercase hex.
pub(crate) fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(char::from(DIGITS[usize::from(b >> 4)]));
        out.push(char::from(DIGITS[usize::from(b & 0x0f)]));
    }
    out
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    /// Fills with 0, 1, 2, … so tokens are predictable.
    struct Counting(Cell<u8>);

    impl Random for Counting {
        fn fill(&self, bytes: &mut [u8]) {
            for b in bytes {
                *b = self.0.get();
                self.0.set(self.0.get().wrapping_add(1));
            }
        }
    }

    #[test]
    fn a_token_is_its_prefix_and_64_hex_digits() {
        let token = NewToken::generate(TokenKind::SecretKey, &Counting(Cell::new(0)));
        assert_eq!(
            token.expose(),
            "hypha_sk_000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f"
        );
        assert_eq!(token.last_four(), "1e1f");
        assert_eq!(token.hash(), TokenHash::of(token.expose()));
    }

    #[test]
    fn kind_reads_the_prefix_and_checks_the_shape() {
        let random = Counting(Cell::new(7));
        for kind in TokenKind::ALL {
            let token = NewToken::generate(kind, &random);
            assert_eq!(TokenKind::of(token.expose()), Some(kind));
        }
        let session = NewToken::generate(TokenKind::Session, &random);
        let body = session.expose().strip_prefix("hypha_sess_").unwrap();
        assert_eq!(
            TokenKind::of(&format!("hypha_sk_{body}")),
            Some(TokenKind::SecretKey)
        );
        assert_eq!(TokenKind::of(&format!("hypha_xx_{body}")), None);
        assert_eq!(TokenKind::of(&format!("hypha_sk_{}", &body[1..])), None);
        assert_eq!(
            TokenKind::of(&format!("hypha_sk_{}", body.to_uppercase())),
            None
        );
    }

    #[test]
    fn debug_never_shows_the_secret() {
        let token = NewToken::generate(TokenKind::Session, &Counting(Cell::new(0)));
        assert!(!format!("{token:?}").contains(token.expose()));
        assert!(!format!("{token:?}").contains("000102"));
    }
}
