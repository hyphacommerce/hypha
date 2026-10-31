//! AWS Signature Version 4 for S3: the two forms Hypha needs, a presigned URL (the
//! signature in the query string, for clients) and an `Authorization` header (for
//! Hypha's own requests). HMAC-SHA256 only, so it runs anywhere, Workers included.
//!
//! Reference: "Authenticating Requests (AWS Signature Version 4)" in the S3 API docs.

use hmac::{Hmac, KeyInit, Mac};
use hypha_core::Timestamp;
use sha2::{Digest, Sha256};

/// The SHA-256 of an empty body, which every Hypha request without a body sends.
pub(crate) const EMPTY_SHA256: &str =
    "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

pub(crate) struct Signer<'a> {
    pub access_key_id: &'a str,
    pub secret_access_key: &'a str,
    pub region: &'a str,
}

/// A request to sign: `headers` are the ones to sign besides `host` (names lowercase).
pub(crate) struct Request<'a> {
    pub method: &'a str,
    pub host: &'a str,
    /// Already encoded (`encode_path`).
    pub path: &'a str,
    pub headers: &'a [(String, String)],
    pub now: Timestamp,
}

impl Signer<'_> {
    /// The query string of a presigned URL valid for `expires_in_secs` (without `?`).
    /// The client must send exactly `request.headers`.
    pub(crate) fn presign(&self, request: &Request<'_>, expires_in_secs: u32) -> String {
        let (date, time) = amz_date(request.now);
        let scope = format!("{date}/{}/s3/aws4_request", self.region);
        let headers = with_host(request.host, request.headers);
        let signed_headers = signed_header_names(&headers);
        let mut query = vec![
            (
                "X-Amz-Algorithm".to_string(),
                "AWS4-HMAC-SHA256".to_string(),
            ),
            (
                "X-Amz-Credential".to_string(),
                format!("{}/{scope}", self.access_key_id),
            ),
            ("X-Amz-Date".to_string(), time.clone()),
            ("X-Amz-Expires".to_string(), expires_in_secs.to_string()),
            ("X-Amz-SignedHeaders".to_string(), signed_headers.clone()),
        ];
        let unsigned_query = canonical_query(&query);
        let canonical = canonical_request(
            request.method,
            request.path,
            &unsigned_query,
            &headers,
            &signed_headers,
            "UNSIGNED-PAYLOAD",
        );
        let signature = self.signature(&date, &time, &scope, &canonical);
        query.push(("X-Amz-Signature".to_string(), signature));
        canonical_query(&query)
    }

    /// The headers to add to `request` (`x-amz-date`, `x-amz-content-sha256`,
    /// `authorization`) for a request without a body.
    pub(crate) fn authorize(&self, request: &Request<'_>) -> Vec<(String, String)> {
        let (date, time) = amz_date(request.now);
        let scope = format!("{date}/{}/s3/aws4_request", self.region);
        let mut own = request.headers.to_vec();
        own.push(("x-amz-content-sha256".to_string(), EMPTY_SHA256.to_string()));
        own.push(("x-amz-date".to_string(), time.clone()));
        let headers = with_host(request.host, &own);
        let signed_headers = signed_header_names(&headers);
        let canonical = canonical_request(
            request.method,
            request.path,
            "",
            &headers,
            &signed_headers,
            EMPTY_SHA256,
        );
        let signature = self.signature(&date, &time, &scope, &canonical);
        vec![
            ("x-amz-content-sha256".to_string(), EMPTY_SHA256.to_string()),
            ("x-amz-date".to_string(), time),
            (
                "authorization".to_string(),
                format!(
                    "AWS4-HMAC-SHA256 Credential={}/{scope},SignedHeaders={signed_headers},Signature={signature}",
                    self.access_key_id
                ),
            ),
        ]
    }

    fn signature(&self, date: &str, time: &str, scope: &str, canonical: &str) -> String {
        let to_sign = format!(
            "AWS4-HMAC-SHA256\n{time}\n{scope}\n{}",
            hex(&Sha256::digest(canonical.as_bytes()))
        );
        let key = hmac(format!("AWS4{}", self.secret_access_key).as_bytes(), date);
        let key = hmac(&key, self.region);
        let key = hmac(&key, "s3");
        let key = hmac(&key, "aws4_request");
        hex(&hmac(&key, &to_sign))
    }
}

fn hmac(key: &[u8], data: &str) -> Vec<u8> {
    #[expect(clippy::expect_used, reason = "HMAC takes keys of any length")]
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(data.as_bytes());
    mac.finalize().into_bytes().to_vec()
}

/// `headers` plus `host`, lowercase names, values trimmed, sorted by name.
fn with_host(host: &str, headers: &[(String, String)]) -> Vec<(String, String)> {
    let mut all: Vec<(String, String)> = headers
        .iter()
        .map(|(name, value)| (name.to_ascii_lowercase(), value.trim().to_string()))
        .collect();
    all.push(("host".to_string(), host.to_string()));
    all.sort();
    all
}

fn signed_header_names(headers: &[(String, String)]) -> String {
    headers
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>()
        .join(";")
}

fn canonical_request(
    method: &str,
    path: &str,
    query: &str,
    headers: &[(String, String)],
    signed_headers: &str,
    payload_hash: &str,
) -> String {
    let headers: String = headers
        .iter()
        .map(|(name, value)| format!("{name}:{value}\n"))
        .collect();
    format!("{method}\n{path}\n{query}\n{headers}\n{signed_headers}\n{payload_hash}")
}

/// Query parameters encoded and sorted, as the canonical request has them.
fn canonical_query(params: &[(String, String)]) -> String {
    let mut encoded: Vec<(String, String)> = params
        .iter()
        .map(|(k, v)| (encode(k, true), encode(v, true)))
        .collect();
    encoded.sort();
    encoded
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&")
}

/// An object key as a URL path: each byte outside `A-Za-z0-9-._~` percent-encoded, `/`
/// kept between segments.
pub(crate) fn encode_path(path: &str) -> String {
    encode(path, false)
}

/// S3's URI encoding: unreserved characters as they are, everything else `%XX`; `/`
/// too unless it separates path segments.
fn encode(s: &str, encode_slash: bool) -> String {
    let mut out = String::new();
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char);
            }
            b'/' if !encode_slash => out.push('/'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// `("YYYYMMDD", "YYYYMMDDTHHMMSSZ")` in UTC.
pub(crate) fn amz_date(at: Timestamp) -> (String, String) {
    let secs = at.unix_secs();
    let (days, rest) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let (year, month, day) = civil_from_days(days);
    let date = format!("{year:04}{month:02}{day:02}");
    let time = format!(
        "{date}T{:02}{:02}{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    );
    (date, time)
}

/// Days since 1970-01-01 to a proleptic Gregorian `(year, month, day)` (Howard
/// Hinnant's `civil_from_days`), so no date library is needed.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The credentials and moment of the S3 docs' signing examples (2013-05-24T00:00:00Z).
    fn example() -> (Signer<'static>, Timestamp) {
        (
            Signer {
                access_key_id: "AKIAIOSFODNN7EXAMPLE",
                secret_access_key: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
                region: "us-east-1",
            },
            Timestamp::from_unix_micros(1_369_353_600_000_000),
        )
    }

    #[test]
    fn dates_are_formatted_in_utc() {
        let (_, now) = example();
        assert_eq!(
            amz_date(now),
            ("20130524".to_string(), "20130524T000000Z".to_string())
        );
        let leap = Timestamp::from_unix_micros(951_782_400_000_000 + 3_723_000_000);
        assert_eq!(amz_date(leap).1, "20000229T010203Z");
    }

    #[test]
    fn a_presigned_url_matches_the_s3_docs_example() {
        // "Example: Presigned URL" (GET /test.txt, valid for 86400 seconds).
        let (signer, now) = example();
        let query = signer.presign(
            &Request {
                method: "GET",
                host: "examplebucket.s3.amazonaws.com",
                path: "/test.txt",
                headers: &[],
                now,
            },
            86_400,
        );
        assert!(
            query.contains(
                "&X-Amz-Signature=aeeed9bbccd4d02ee5c0109b86d86835f995330da4c265957d157751f604d404&"
            ),
            "{query}"
        );
    }

    #[test]
    fn an_authorization_header_matches_the_s3_docs_example() {
        // "Example: GET Object" (the first 10 bytes of /test.txt).
        let (signer, now) = example();
        let headers = signer.authorize(&Request {
            method: "GET",
            host: "examplebucket.s3.amazonaws.com",
            path: "/test.txt",
            headers: &[("range".to_string(), "bytes=0-9".to_string())],
            now,
        });
        let authorization = &headers
            .iter()
            .find(|(n, _)| n == "authorization")
            .unwrap()
            .1;
        assert_eq!(
            authorization,
            "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request,\
             SignedHeaders=host;range;x-amz-content-sha256;x-amz-date,\
             Signature=f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41"
        );
    }

    #[test]
    fn paths_keep_slashes_and_encode_the_rest() {
        assert_eq!(encode_path("/bucket/a b/ç.png"), "/bucket/a%20b/%C3%A7.png");
        assert_eq!(encode("a/b", true), "a%2Fb");
    }
}
