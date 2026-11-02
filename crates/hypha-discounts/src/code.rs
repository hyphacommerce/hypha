//! Discount codes: rows of their own, many per discount, unique in the store by their
//! normalised text.

use hypha_core::{DiscountId, Random, Timestamp, optional_text};
use unicode_normalization::UnicodeNormalization;

use crate::error::CodeError;

/// Most characters in a code.
pub const MAX_CODE_LEN: usize = 64;
/// Generated codes' length, prefix included.
pub const MIN_GENERATED_LEN: usize = 8;
pub const MAX_GENERATED_LEN: usize = 24;
/// Fewest random characters after a generated code's prefix: about 40 bits, so codes
/// can't be guessed within the rate limits however short the prefix leaves them.
pub const MIN_RANDOM_LEN: usize = 8;
/// Most codes generated in one call.
pub const MAX_GENERATED: usize = 10_000;
/// Most codes added in one call.
pub const MAX_ADDED: usize = 1_000;
/// What generated codes are drawn from: letters and digits without the ones read as each
/// other (`0`/`O`, `1`/`I`/`L`).
pub const ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";

/// A code as compared and stored: trimmed, compatibility-normalised (NFKC, so a
/// full-width `Ｗ` is `W`) and upper-cased, so `welcome10 ` and `WELCOME10` are one
/// code. 1 to `MAX_CODE_LEN` characters, without spaces or control characters.
pub fn normalize(raw: &str) -> Result<String, CodeError> {
    let folded: String = raw.trim().nfkc().collect::<String>().to_uppercase();
    let code: String = folded.nfkc().collect();
    let length = code.chars().count();
    if length == 0 || length > MAX_CODE_LEN {
        return Err(CodeError::Length);
    }
    if code.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(CodeError::Character(raw.to_string()));
    }
    Ok(code)
}

/// One code of a discount.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscountCode {
    pub discount: DiscountId,
    /// As the merchant typed it, for showing.
    pub code: String,
    /// `normalize(code)`: the key.
    pub normalized: String,
    /// Orders it may be used on; 1 for a single-use code, `None` for no limit beyond the
    /// discount's.
    pub usage_limit: Option<u32>,
    /// An inactive code is refused like an unknown one.
    pub active: bool,
    /// A label for a generated set, to remove or export it together.
    pub batch: Option<String>,
    /// Orders it was used on (read; written only by checkout).
    pub used: u64,
    /// When it was added (read; set by the adapter).
    pub created_at: Option<Timestamp>,
}

impl DiscountCode {
    /// A new, active, unused code.
    pub fn new(
        discount: DiscountId,
        code: &str,
        usage_limit: Option<u32>,
        batch: Option<String>,
    ) -> Result<Self, CodeError> {
        if usage_limit == Some(0) {
            return Err(CodeError::UsageLimit);
        }
        Ok(Self {
            discount,
            code: code.trim().to_string(),
            normalized: normalize(code)?,
            usage_limit,
            active: true,
            batch: batch_label(batch)?,
            used: 0,
            created_at: None,
        })
    }

    /// Whether it has uses left of its own.
    pub fn has_uses_left(&self) -> bool {
        self.usage_limit
            .is_none_or(|limit| self.used < u64::from(limit))
    }
}

/// A batch label: trimmed, 1 to 255 characters; blank is none.
pub fn batch_label(batch: Option<String>) -> Result<Option<String>, CodeError> {
    optional_text("batch", batch).map_err(|_| CodeError::Batch)
}

/// How codes are generated: `count` of them, `length` characters each, starting with
/// `prefix` (normalised like any code).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Generation {
    pub count: usize,
    pub prefix: Option<String>,
    pub length: usize,
}

impl Generation {
    /// Checks the counts and lengths, and normalises the prefix.
    pub fn check(self) -> Result<Self, CodeError> {
        if self.count == 0 || self.count > MAX_GENERATED {
            return Err(CodeError::TooMany);
        }
        let prefix = match self.prefix.as_deref().map(str::trim) {
            None | Some("") => None,
            Some(p) => Some(normalize(p)?),
        };
        let prefix_len = prefix.as_ref().map_or(0, |p| p.chars().count());
        if !(MIN_GENERATED_LEN..=MAX_GENERATED_LEN).contains(&self.length)
            || prefix_len + MIN_RANDOM_LEN > self.length
        {
            return Err(CodeError::GeneratedLength);
        }
        Ok(Self { prefix, ..self })
    }

    /// One code at random: the prefix, then letters and digits from `ALPHABET` (at least
    /// `MIN_RANDOM_LEN`, about 40 bits, more with a longer length).
    pub fn draw(&self, random: &impl Random) -> String {
        let prefix = self.prefix.as_deref().unwrap_or("");
        let mut code = String::with_capacity(self.length);
        code.push_str(prefix);
        let wanted = self.length - prefix.chars().count();
        let mut drawn = 0;
        // Bytes past the alphabet's largest multiple are dropped, so every character is
        // equally likely.
        let cutoff = 256 / ALPHABET.len() * ALPHABET.len();
        while drawn < wanted {
            let mut bytes = [0u8; 32];
            random.fill(&mut bytes);
            for b in bytes {
                if drawn == wanted {
                    break;
                }
                if usize::from(b) < cutoff {
                    code.push(ALPHABET[usize::from(b) % ALPHABET.len()] as char);
                    drawn += 1;
                }
            }
        }
        code
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    #[test]
    fn a_code_is_trimmed_and_upper_cased() {
        assert_eq!(normalize("  welcome10 "), Ok("WELCOME10".to_string()));
    }

    #[test]
    fn full_width_characters_fold_to_their_plain_forms() {
        assert_eq!(normalize("ｗｅｌｃｏｍｅ１０"), Ok("WELCOME10".to_string()));
    }

    #[test]
    fn a_blank_code_is_refused() {
        assert_eq!(normalize("   "), Err(CodeError::Length));
    }

    #[test]
    fn a_code_of_the_longest_length_is_kept_and_one_more_refused() {
        let max = "A".repeat(MAX_CODE_LEN);
        assert_eq!(normalize(&max), Ok(max.clone()));
        assert_eq!(normalize(&format!("{max}A")), Err(CodeError::Length));
    }

    #[test]
    fn a_space_inside_a_code_is_refused() {
        assert_eq!(
            normalize("WEL COME"),
            Err(CodeError::Character("WEL COME".to_string()))
        );
    }

    #[test]
    fn a_control_character_is_refused() {
        assert_eq!(
            normalize("A\u{7}B"),
            Err(CodeError::Character("A\u{7}B".to_string()))
        );
    }

    #[test]
    fn a_new_code_keeps_what_was_typed_beside_its_key() {
        let code = DiscountCode::new(
            DiscountId::from_uuid(uuid::Uuid::from_u128(1)),
            " Summer ",
            Some(1),
            None,
        )
        .unwrap();
        assert_eq!(code.code, "Summer");
        assert_eq!(code.normalized, "SUMMER");
    }

    #[test]
    fn a_usage_limit_of_zero_is_refused() {
        assert_eq!(
            DiscountCode::new(
                DiscountId::from_uuid(uuid::Uuid::from_u128(1)),
                "A",
                Some(0),
                None
            ),
            Err(CodeError::UsageLimit)
        );
    }

    #[test]
    fn a_single_use_code_has_no_uses_left_once_used() {
        let mut code = DiscountCode::new(
            DiscountId::from_uuid(uuid::Uuid::from_u128(1)),
            "A",
            Some(1),
            None,
        )
        .unwrap();
        assert!(code.has_uses_left());
        code.used = 1;
        assert!(!code.has_uses_left());
    }

    /// Bytes counting up from `start`.
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
    fn generated_codes_have_the_length_asked_with_the_prefix_first() {
        let generation = Generation {
            count: 1,
            prefix: Some("vip-".to_string()),
            length: 12,
        }
        .check()
        .unwrap();
        let code = generation.draw(&Counting(Cell::new(0)));
        assert_eq!(code, "VIP-ABCDEFGH");
    }

    #[test]
    fn generated_codes_skip_bytes_past_the_alphabet() {
        // 248 and up are dropped: 31 letters fit 8 times in 256.
        let generation = Generation {
            count: 1,
            prefix: None,
            length: 8,
        }
        .check()
        .unwrap();
        let code = generation.draw(&Counting(Cell::new(246)));
        // 246 % 31 = 29 ('8'), 247 → '9', then 248..=255 dropped, 0 → 'A'…
        assert_eq!(code, "89ABCDEF");
    }

    #[test]
    fn generated_codes_use_no_look_alikes() {
        for c in *b"0O1IL" {
            assert!(!ALPHABET.contains(&c));
        }
    }

    #[test]
    fn a_generated_length_outside_the_range_is_refused() {
        for length in [MIN_GENERATED_LEN - 1, MAX_GENERATED_LEN + 1] {
            let generation = Generation {
                count: 1,
                prefix: None,
                length,
            };
            assert_eq!(generation.check(), Err(CodeError::GeneratedLength));
        }
    }

    #[test]
    fn a_prefix_leaves_room_for_eight_random_characters() {
        let short = Generation {
            count: 1,
            prefix: Some("SUMMER".to_string()),
            length: 13,
        };
        assert_eq!(short.clone().check(), Err(CodeError::GeneratedLength));
        let enough = Generation {
            length: 14,
            ..short
        };
        assert_eq!(enough.check().map(|g| g.length), Ok(14));
    }

    #[test]
    fn more_codes_than_the_most_at_once_are_refused() {
        let generation = Generation {
            count: MAX_GENERATED + 1,
            prefix: None,
            length: 12,
        };
        assert_eq!(generation.check(), Err(CodeError::TooMany));
    }
}
