//! Recovery codes: single-use second factors for when the authenticator app is lost.

use hypha_core::Random;

use crate::{token::TokenHash, totp::base32};

/// How many codes a set has.
pub const CODES_PER_SET: usize = 10;
/// Random bytes per code: 80 bits, 16 base32 characters. Enough that SHA-256 storage is
/// safe (like tokens, unlike passwords), short enough to type.
const CODE_BYTES: usize = 10;

/// A new set of codes: what to show the user (once) and what to store.
pub struct NewRecoveryCodes {
    pub codes: Vec<String>,
    pub hashes: Vec<TokenHash>,
}

impl std::fmt::Debug for NewRecoveryCodes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "NewRecoveryCodes({} codes)", self.codes.len())
    }
}

pub fn generate(random: &impl Random) -> NewRecoveryCodes {
    let codes: Vec<String> = (0..CODES_PER_SET)
        .map(|_| {
            let mut bytes = [0u8; CODE_BYTES];
            random.fill(&mut bytes);
            let plain = base32(&bytes);
            // ABCD-EFGH-IJKL-MNOP: easier to read off and type.
            plain
                .as_bytes()
                .chunks(4)
                .map(|chunk| String::from_utf8_lossy(chunk).into_owned())
                .collect::<Vec<_>>()
                .join("-")
        })
        .collect();
    let hashes = codes.iter().filter_map(|code| hash(code)).collect();
    NewRecoveryCodes { codes, hashes }
}

/// The stored form of a code as typed: dashes and spaces dropped, case ignored. `None`
/// if what's left can't be a code.
pub fn hash(typed: &str) -> Option<TokenHash> {
    let normalized: String = typed
        .chars()
        .filter(|c| !matches!(c, '-' | ' '))
        .map(|c| c.to_ascii_uppercase())
        .collect();
    let valid = normalized.len() == CODE_BYTES * 8 / 5
        && normalized
            .bytes()
            .all(|b| b.is_ascii_uppercase() || (b'2'..=b'7').contains(&b));
    valid.then(|| TokenHash::of(&normalized))
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

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
    fn a_set_has_ten_distinct_codes_in_groups_of_four() {
        let set = generate(&Counting(Cell::new(0)));
        assert_eq!(set.codes.len(), 10);
        assert_eq!(set.hashes.len(), 10);
        assert_eq!(set.codes[0], "AAAQ-EAYE-AUDA-OCAJ");
        let mut unique = set.codes.clone();
        unique.dedup();
        assert_eq!(unique.len(), 10);
    }

    #[test]
    fn a_code_hashes_the_same_however_it_is_typed() {
        let set = generate(&Counting(Cell::new(0)));
        assert_eq!(hash("AAAQ-EAYE-AUDA-OCAJ"), Some(set.hashes[0]));
        assert_eq!(hash("aaaq eaye auda ocaj"), Some(set.hashes[0]));
        assert_eq!(hash("AAAQEAYEAUDAOCAJ"), Some(set.hashes[0]));
    }

    #[test]
    fn junk_is_not_a_code() {
        for junk in [
            "",
            "AAAQ-EAYE-AUDA-OCA",
            "AAAQ-EAYE-AUDA-OCAJA",
            "AAAQ-EAYE-AUDA-OCA1",
        ] {
            assert_eq!(hash(junk), None, "{junk:?}");
        }
    }
}
