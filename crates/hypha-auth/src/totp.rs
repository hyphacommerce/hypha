//! Time-based one-time passwords (RFC 6238), as authenticator apps compute them:
//! HMAC-SHA1 over the number of 30-second steps since the epoch, cut to 6 digits.
//!
//! SHA-1 is fine here: HMAC doesn't rely on the collision resistance SHA-1 lost, and
//! it's what every authenticator app supports.

use hmac::{Hmac, KeyInit, Mac};
use hypha_core::Timestamp;
use sha1::Sha1;

/// Seconds per step.
pub const STEP_SECS: i64 = 30;
/// Bytes in a new secret: 160 bits, as RFC 4226 recommends.
pub const SECRET_BYTES: usize = 20;
const DIGITS: u32 = 6;

/// The step `now` falls in. Times before 1970 count as step 0.
pub fn step_at(now: Timestamp) -> u64 {
    u64::try_from(now.unix_secs().div_euclid(STEP_SECS)).unwrap_or(0)
}

/// The code for `step` (HOTP, RFC 4226 section 5.3).
pub fn code_at(secret: &[u8], step: u64) -> u32 {
    // HMAC takes keys of any length, so this can't fail.
    let mut mac = Hmac::<Sha1>::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(&step.to_be_bytes());
    let digest = mac.finalize().into_bytes();
    // Dynamic truncation: the low 4 bits of the last byte pick 4 bytes to read.
    let offset = usize::from(digest[19] & 0x0f);
    let value = u32::from_be_bytes([
        digest[offset] & 0x7f,
        digest[offset + 1],
        digest[offset + 2],
        digest[offset + 3],
    ]);
    value % 10u32.pow(DIGITS)
}

/// What a code is, checked against the steps around `now`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    /// Right, for this step, which hasn't been used yet.
    Valid(u64),
    /// Right, but for a step at or before `last_used`: someone (the user, a double
    /// submit, or whoever watched it typed) already used it. Refused, but not a guess.
    Replayed,
    Wrong,
}

/// Checks `code` against one step either side of `now` (clock drift). A step at or
/// before `last_used` is `Replayed`, so a code can't be used twice.
pub fn check(secret: &[u8], code: &str, now: Timestamp, last_used: Option<u64>) -> Check {
    let code = code.trim();
    if code.len() != DIGITS as usize || !code.bytes().all(|b| b.is_ascii_digit()) {
        return Check::Wrong;
    }
    let Ok(wanted) = code.parse::<u32>() else {
        return Check::Wrong;
    };
    let current = step_at(now);
    let mut replayed = false;
    for step in [
        current.saturating_sub(1),
        current,
        current.saturating_add(1),
    ] {
        if !constant_time_eq(code_at(secret, step), wanted) {
            continue;
        }
        if last_used.is_none_or(|last| step > last) {
            return Check::Valid(step);
        }
        replayed = true;
    }
    if replayed {
        Check::Replayed
    } else {
        Check::Wrong
    }
}

/// Compares without an early exit, so timing doesn't tell how close a guess was.
fn constant_time_eq(a: u32, b: u32) -> bool {
    (a ^ b) == 0
}

/// RFC 4648 base32 without padding: how authenticator apps take a secret typed in.
pub fn base32(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut out = String::with_capacity(bytes.len().div_ceil(5) * 8);
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for &byte in bytes {
        buffer = (buffer << 8) | u32::from(byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(char::from(ALPHABET[((buffer >> bits) & 31) as usize]));
        }
    }
    if bits > 0 {
        out.push(char::from(ALPHABET[((buffer << (5 - bits)) & 31) as usize]));
    }
    out
}

/// Reads `base32` output back (case-insensitive, padding and spaces ignored). `None`
/// for any other character.
pub fn from_base32(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 5 / 8);
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for c in text.chars().filter(|c| !matches!(c, '=' | ' ')) {
        let value = match c.to_ascii_uppercase() {
            c @ 'A'..='Z' => u32::from(c) - u32::from('A'),
            c @ '2'..='7' => u32::from(c) - u32::from('2') + 26,
            _ => return None,
        };
        buffer = (buffer << 5) | value;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push(((buffer >> bits) & 0xff) as u8);
        }
    }
    Some(out)
}

/// The `otpauth://` URI an authenticator app reads from a QR code. `issuer` (the store)
/// and `account` (the email) label the entry in the app.
pub fn otpauth_uri(issuer: &str, account: &str, secret: &[u8]) -> String {
    format!(
        "otpauth://totp/{}:{}?secret={}&issuer={}&algorithm=SHA1&digits={DIGITS}&period={STEP_SECS}",
        percent_encode(issuer),
        percent_encode(account),
        base32(secret),
        percent_encode(issuer),
    )
}

/// Percent-encodes everything but RFC 3986's unreserved characters.
fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 6238 appendix B's SHA-1 secret.
    const RFC_SECRET: &[u8] = b"12345678901234567890";

    fn at(secs: i64) -> Timestamp {
        Timestamp::from_unix_micros(secs * 1_000_000)
    }

    #[test]
    fn matches_the_rfc_6238_test_vectors() {
        // The RFC lists 8 digits; the last 6 are the 6-digit code.
        for (secs, code) in [
            (59, 287_082),
            (1_111_111_109, 81_804),
            (1_111_111_111, 50_471),
            (1_234_567_890, 5_924),
            (2_000_000_000, 279_037),
        ] {
            assert_eq!(code_at(RFC_SECRET, step_at(at(secs))), code, "{secs}");
        }
    }

    #[test]
    fn accepts_one_step_of_drift_either_way_and_no_more() {
        let now = at(1_111_111_111);
        let step = step_at(now);
        for (offset, ok) in [(-2i64, false), (-1, true), (0, true), (1, true), (2, false)] {
            let s = step.checked_add_signed(offset).unwrap();
            let code = format!("{:06}", code_at(RFC_SECRET, s));
            let want = if ok { Check::Valid(s) } else { Check::Wrong };
            assert_eq!(check(RFC_SECRET, &code, now, None), want, "{offset}");
        }
    }

    #[test]
    fn a_used_code_is_replayed_not_wrong() {
        let now = at(1_111_111_111);
        let step = step_at(now);
        let code = format!("{:06}", code_at(RFC_SECRET, step));
        assert_eq!(check(RFC_SECRET, &code, now, Some(step)), Check::Replayed);
        assert_eq!(
            check(RFC_SECRET, &code, now, Some(step - 1)),
            Check::Valid(step)
        );
        assert_eq!(check(RFC_SECRET, "000000", now, Some(step)), Check::Wrong);
    }

    #[test]
    fn refuses_codes_that_are_not_six_digits() {
        let now = at(59);
        assert_eq!(check(RFC_SECRET, " 287082 ", now, None), Check::Valid(1));
        for bad in ["28708", "2870820", "28708a", "", "+87082"] {
            assert_eq!(check(RFC_SECRET, bad, now, None), Check::Wrong, "{bad:?}");
        }
    }

    #[test]
    fn base32_matches_rfc_4648() {
        for (input, output) in [
            ("", ""),
            ("f", "MY"),
            ("fo", "MZXQ"),
            ("foo", "MZXW6"),
            ("foob", "MZXW6YQ"),
            ("fooba", "MZXW6YTB"),
            ("foobar", "MZXW6YTBOI"),
        ] {
            assert_eq!(base32(input.as_bytes()), output);
            assert_eq!(from_base32(output), Some(input.as_bytes().to_vec()));
        }
        assert_eq!(from_base32("mzxw6"), Some(b"foo".to_vec()));
        assert_eq!(from_base32("MZXW1"), None);
    }

    #[test]
    fn the_uri_labels_the_entry_and_carries_the_secret() {
        assert_eq!(
            otpauth_uri("My Shop", "ana@example.com", b"foobar"),
            "otpauth://totp/My%20Shop:ana%40example.com?secret=MZXW6YTBOI&issuer=My%20Shop\
             &algorithm=SHA1&digits=6&period=30"
        );
    }
}
