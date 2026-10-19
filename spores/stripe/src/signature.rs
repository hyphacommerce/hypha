//! Stripe's webhook signatures (<https://docs.stripe.com/webhooks#verify-manually>):
//! `Stripe-Signature: t=<unix>,v1=<hex HMAC-SHA256 of "<t>.<raw body>">[,v1=…][,v0=…]`,
//! keyed with the endpoint's signing secret (`whsec_…`, used as is). Only `v1` counts; a
//! timestamp more than five minutes away is a replay.

use hmac::{Hmac, KeyInit, Mac};

/// The header Stripe signs with.
pub const HEADER: &str = "stripe-signature";
/// How far a signature's time may be from now, in seconds.
pub const TOLERANCE_SECONDS: i64 = 300;

/// Whether `header` signs `body` with `secret`, at `now` (Unix seconds).
pub fn verify(secret: &str, header: Option<&str>, body: &[u8], now: i64) -> bool {
    let Some(header) = header.filter(|_| !secret.is_empty()) else {
        return false;
    };
    let mut timestamp: Option<i64> = None;
    let mut candidates: Vec<Vec<u8>> = Vec::new();
    for part in header.split(',') {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        match key.trim() {
            "t" => timestamp = value.trim().parse().ok(),
            "v1" => {
                if let Some(bytes) = from_hex(value.trim()) {
                    candidates.push(bytes);
                }
            }
            _ => {}
        }
    }
    let Some(timestamp) = timestamp else {
        return false;
    };
    if candidates.is_empty() || (now - timestamp).abs() > TOLERANCE_SECONDS {
        return false;
    }
    // Every candidate is checked, each in constant time, so the time taken doesn't say
    // which one matched.
    let mut matched = false;
    for candidate in &candidates {
        if mac(secret, timestamp, body).verify_slice(candidate).is_ok() {
            matched = true;
        }
    }
    matched
}

/// The `Stripe-Signature` value Stripe would send: for tests and a fake Stripe.
pub fn sign(secret: &str, body: &[u8], timestamp: i64) -> String {
    let digest = mac(secret, timestamp, body).finalize().into_bytes();
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("t={timestamp},v1={hex}")
}

fn mac(secret: &str, timestamp: i64, body: &[u8]) -> Hmac<sha2::Sha256> {
    // An HMAC takes a key of any length: this can't fail.
    let mut mac = Hmac::<sha2::Sha256>::new_from_slice(secret.as_bytes())
        .unwrap_or_else(|_| unreachable!("HMAC keys may be any length"));
    mac.update(format!("{timestamp}.").as_bytes());
    mac.update(body);
    mac
}

/// 64 lowercase hex digits as 32 bytes.
fn from_hex(text: &str) -> Option<Vec<u8>> {
    if text.len() != 64 {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "whsec_test";
    const BODY: &[u8] = br#"{"id":"evt_1"}"#;

    #[test]
    fn a_signature_it_made_verifies() {
        let header = sign(SECRET, BODY, 1_700_000_000);
        assert!(verify(SECRET, Some(&header), BODY, 1_700_000_100));
    }

    #[test]
    fn another_body_or_secret_does_not() {
        let header = sign(SECRET, BODY, 1_700_000_000);
        assert!(!verify(SECRET, Some(&header), b"{}", 1_700_000_000));
        assert!(!verify("whsec_other", Some(&header), BODY, 1_700_000_000));
        assert!(!verify("", Some(&header), BODY, 1_700_000_000));
        assert!(!verify(SECRET, None, BODY, 1_700_000_000));
    }

    #[test]
    fn a_stale_or_future_timestamp_is_a_replay() {
        let header = sign(SECRET, BODY, 1_700_000_000);
        assert!(verify(SECRET, Some(&header), BODY, 1_700_000_300));
        assert!(!verify(SECRET, Some(&header), BODY, 1_700_000_301));
        assert!(!verify(SECRET, Some(&header), BODY, 1_699_999_699));
    }

    #[test]
    fn any_v1_may_match_and_v0_is_ignored() {
        let good = sign(SECRET, BODY, 1_700_000_000);
        let v1 = good.split_once("v1=").map(|(_, v)| v).unwrap();
        let header = format!("t=1700000000,v0={v1},v1={},v1={v1}", "0".repeat(64));
        assert!(verify(SECRET, Some(&header), BODY, 1_700_000_000));
        let only_v0 = format!("t=1700000000,v0={v1}");
        assert!(!verify(SECRET, Some(&only_v0), BODY, 1_700_000_000));
    }
}
