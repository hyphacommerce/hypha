//! PKCE (RFC 7636) with `S256`, the only method Hypha accepts (OAuth 2.1): the app
//! sends `code_challenge = BASE64URL(SHA256(code_verifier))` when it asks for a code,
//! and the verifier when it exchanges the code, so a stolen code is useless without
//! the verifier that never left the app.

use sha2::{Digest, Sha256};

/// Whether `challenge` has an S256 challenge's shape: 43 base64url characters (a
/// SHA-256 without padding).
pub fn is_challenge(challenge: &str) -> bool {
    challenge.len() == 43 && challenge.bytes().all(is_base64url)
}

/// Whether `verifier` is one RFC 7636 allows: 43 to 128 unreserved characters.
pub fn is_verifier(verifier: &str) -> bool {
    (43..=128).contains(&verifier.len())
        && verifier
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~'))
}

/// The S256 challenge of `verifier`.
pub fn challenge_of(verifier: &str) -> String {
    base64url(&Sha256::digest(verifier.as_bytes()))
}

/// Whether `verifier` answers `challenge`, compared without stopping at the first
/// difference.
pub fn verify(verifier: &str, challenge: &str) -> bool {
    if !is_verifier(verifier) {
        return false;
    }
    let expected = challenge_of(verifier);
    expected.len() == challenge.len()
        && expected
            .bytes()
            .zip(challenge.bytes())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
}

fn is_base64url(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'-' || b == b'_'
}

/// Base64url without padding (RFC 4648 §5).
fn base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        let take = chunk.len() + 1;
        for i in 0..take {
            out.push(char::from(ALPHABET[((n >> (18 - 6 * i)) & 0x3f) as usize]));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 7636, appendix B.
    const VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
    const CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";

    #[test]
    fn the_rfcs_example_verifies() {
        assert_eq!(challenge_of(VERIFIER), CHALLENGE);
        assert!(verify(VERIFIER, CHALLENGE));
        assert!(is_challenge(CHALLENGE));
    }

    #[test]
    fn another_verifier_does_not() {
        let other = format!("{}A", &VERIFIER[..VERIFIER.len() - 1]);
        assert!(!verify(&other, CHALLENGE));
        assert!(!verify(VERIFIER, &CHALLENGE[1..]));
    }

    #[test]
    fn verifiers_are_43_to_128_unreserved_characters() {
        assert!(is_verifier(&"a".repeat(43)));
        assert!(is_verifier(&"a".repeat(128)));
        assert!(!is_verifier(&"a".repeat(42)));
        assert!(!is_verifier(&"a".repeat(129)));
        assert!(!is_verifier(&format!("{}+", "a".repeat(42))));
        assert!(!verify(&"a".repeat(42), &challenge_of(&"a".repeat(42))));
    }

    #[test]
    fn base64url_pads_nothing() {
        assert_eq!(base64url(b""), "");
        assert_eq!(base64url(b"f"), "Zg");
        assert_eq!(base64url(b"fo"), "Zm8");
        assert_eq!(base64url(b"foo"), "Zm9v");
        assert_eq!(base64url(&[0xfb, 0xff]), "-_8");
    }
}
