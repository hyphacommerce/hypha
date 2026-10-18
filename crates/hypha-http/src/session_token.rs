//! Session tokens for apps' embedded admin pages (design note 0029, §6.3, Q4): a JWT,
//! HS256 under the app's secret, a minute long, its audience the app. The admin asks
//! for one (`POST /admin/app-extensions/{app}/session`) and hands it to the app through
//! the bridge; the app verifies it with `@hypha/app`'s `verifySessionToken`
//! (`docs/apps/bridge.md`). The tests below and that package's share a vector.

use serde::Serialize;

/// How long a token works.
pub const SESSION_TOKEN_SECONDS: i64 = 60;

/// The claims, in the order they're written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SessionClaims {
    /// The API's base URL.
    pub iss: String,
    /// The app's key.
    pub aud: String,
    /// The staff member's user ID.
    pub sub: String,
    /// The store's ID.
    pub store: String,
    /// The app's scopes as far as the staff member holds them, space-separated.
    pub scope: String,
    pub iat: i64,
    pub exp: i64,
    pub jti: String,
}

fn base64url(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// The signed token: `base64url(header).base64url(claims).base64url(HMAC-SHA256)`.
pub fn sign_session_token(claims: &SessionClaims, secret: &str) -> String {
    use hmac::{Hmac, KeyInit, Mac};
    let header = base64url(br#"{"alg":"HS256","typ":"JWT"}"#);
    // Strings and integers only: serializing can't fail.
    let payload = serde_json::to_vec(claims)
        .unwrap_or_else(|e| unreachable!("session claims serialize: {e}"));
    let signing_input = format!("{header}.{}", base64url(&payload));
    let mut mac = Hmac::<sha2::Sha256>::new_from_slice(secret.as_bytes())
        .unwrap_or_else(|_| unreachable!("HMAC accepts every key length"));
    mac.update(signing_input.as_bytes());
    let signature = base64url(&mac.finalize().into_bytes());
    format!("{signing_input}.{signature}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The vector `packages/app/src/session-token.test.ts` verifies too.
    const VECTOR: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJodHRwczovL2FwaS5zaG9wLmV4YW1wbGUiLCJhdWQiOiJhcHBfc3RyaXBlIiwic3ViIjoidXNlcl8xIiwic3RvcmUiOiJzdG9yZV8xIiwic2NvcGUiOiJwYXltZW50czp3cml0ZSBvcmRlcnM6cmVhZCIsImlhdCI6MTcwMDAwMDAwMCwiZXhwIjoxNzAwMDAwMDYwLCJqdGkiOiJqMSJ9.yFiKBURN3H-Uakt1ODF8mCu9C-ZMY30pFXFeflc0pv4";

    fn claims() -> SessionClaims {
        SessionClaims {
            iss: "https://api.shop.example".to_string(),
            aud: "app_stripe".to_string(),
            sub: "user_1".to_string(),
            store: "store_1".to_string(),
            scope: "payments:write orders:read".to_string(),
            iat: 1_700_000_000,
            exp: 1_700_000_060,
            jti: "j1".to_string(),
        }
    }

    #[test]
    fn a_token_is_the_shared_vector() {
        assert_eq!(sign_session_token(&claims(), "hypha_shs_test"), VECTOR);
    }

    #[test]
    fn the_header_says_hs256() {
        let token = sign_session_token(&claims(), "s");
        let header = token.split('.').next().unwrap();
        use base64::Engine;
        let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(header)
            .unwrap();
        assert_eq!(decoded, br#"{"alg":"HS256","typ":"JWT"}"#);
    }
}
