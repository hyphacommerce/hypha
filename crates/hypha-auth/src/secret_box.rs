//! Encryption for secrets that must be read back, like TOTP seeds. Passwords and tokens are
//! hashed instead; a TOTP seed can't be, because checking a code needs the seed itself.

use std::fmt;

use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, Payload},
};

// The port and its types live in `hypha-core` (other leaf crates seal secrets too:
// shipping's callback secrets); this crate keeps the cipher.
pub use hypha_core::{SealError, Sealed, SecretBox};

/// XChaCha20-Poly1305 under one 32-byte key. Its 24-byte nonce is long enough to pick at
/// random without ever repeating in practice.
#[derive(Clone)]
pub struct XChaChaBox {
    cipher: XChaCha20Poly1305,
}

impl XChaChaBox {
    const VERSION: u8 = 1;

    pub fn new(key: [u8; 32]) -> Self {
        Self {
            cipher: XChaCha20Poly1305::new(&key.into()),
        }
    }
}

impl fmt::Debug for XChaChaBox {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("XChaChaBox(..)")
    }
}

impl SecretBox for XChaChaBox {
    fn seal(&self, plaintext: &[u8], context: &[u8], nonce: [u8; 24]) -> Sealed {
        let payload = Payload {
            msg: plaintext,
            aad: context,
        };
        #[expect(
            clippy::expect_used,
            reason = "encryption only fails for messages of hundreds of gigabytes"
        )]
        let ciphertext = self
            .cipher
            .encrypt(&XNonce::from(nonce), payload)
            .expect("secrets are small");
        let mut out = Vec::with_capacity(1 + nonce.len() + ciphertext.len());
        out.push(Self::VERSION);
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&ciphertext);
        Sealed::from_bytes(out)
    }

    fn open(&self, sealed: &Sealed, context: &[u8]) -> Result<Vec<u8>, SealError> {
        let (version, rest) = sealed.as_bytes().split_first().ok_or(SealError("empty"))?;
        if *version != Self::VERSION {
            return Err(SealError("unknown format version"));
        }
        let (nonce, ciphertext) = rest
            .split_first_chunk::<24>()
            .ok_or(SealError("truncated"))?;
        let payload = Payload {
            msg: ciphertext,
            aad: context,
        };
        self.cipher
            .decrypt(&XNonce::from(*nonce), payload)
            .map_err(|_| SealError("wrong key, context or data"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sealed() -> (XChaChaBox, Sealed) {
        let b = XChaChaBox::new([1; 32]);
        let s = b.seal(b"seed", b"idn_1", [2; 24]);
        (b, s)
    }

    #[test]
    fn opens_what_it_sealed() {
        let (b, s) = sealed();
        assert_eq!(s.as_bytes()[0], 1);
        assert_eq!(b.open(&s, b"idn_1"), Ok(b"seed".to_vec()));
    }

    #[test]
    fn another_context_or_key_fails() {
        let (b, s) = sealed();
        assert!(b.open(&s, b"idn_2").is_err());
        assert!(XChaChaBox::new([9; 32]).open(&s, b"idn_1").is_err());
    }

    #[test]
    fn tampered_or_truncated_data_fails() {
        let (b, s) = sealed();
        let mut bytes = s.as_bytes().to_vec();
        *bytes.last_mut().unwrap() ^= 1;
        assert!(b.open(&Sealed::from_bytes(bytes), b"idn_1").is_err());
        assert!(
            b.open(&Sealed::from_bytes(vec![1, 2, 3]), b"idn_1")
                .is_err()
        );
        assert!(b.open(&Sealed::from_bytes(vec![]), b"idn_1").is_err());
        let mut other_version = s.as_bytes().to_vec();
        other_version[0] = 2;
        assert!(
            b.open(&Sealed::from_bytes(other_version), b"idn_1")
                .is_err()
        );
    }
}
