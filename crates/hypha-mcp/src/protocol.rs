//! Protocol versions and how a request says which one it speaks.
//!
//! Two eras share the endpoint. A modern request (2026-07-28 on) carries its version
//! and capabilities in `params._meta` and is checked against the HTTP headers that
//! mirror it; there's no handshake. A legacy request (2025-11-25 and before) carries
//! none of that: the client sent `initialize` once and repeats the negotiated version
//! in the `MCP-Protocol-Version` header. The server is stateless either way, so a
//! legacy client's `initialize` is answered and forgotten.

use base64::Engine;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
use serde_json::{Map, Value, json};

use crate::jsonrpc::{HEADER_MISMATCH, INVALID_PARAMS, RpcError, UNSUPPORTED_PROTOCOL_VERSION};

/// The versions served statelessly, per request.
pub const MODERN_VERSIONS: &[&str] = &["2026-07-28"];

/// The versions served to clients that open with `initialize`, newest first.
pub const LEGACY_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26"];

/// What a legacy request without the version header is taken to speak: the header
/// came with 2025-06-18.
pub const UNVERSIONED_LEGACY: &str = "2025-03-26";

pub const PROTOCOL_VERSION_HEADER: &str = "MCP-Protocol-Version";
pub const METHOD_HEADER: &str = "Mcp-Method";
pub const NAME_HEADER: &str = "Mcp-Name";

pub const META_PROTOCOL_VERSION: &str = "io.modelcontextprotocol/protocolVersion";
pub const META_CLIENT_CAPABILITIES: &str = "io.modelcontextprotocol/clientCapabilities";
pub const META_SERVER_INFO: &str = "io.modelcontextprotocol/serverInfo";

/// Every version this server speaks, modern first.
pub fn supported_versions() -> Vec<&'static str> {
    MODERN_VERSIONS
        .iter()
        .chain(LEGACY_VERSIONS)
        .copied()
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Era {
    Modern,
    Legacy,
}

/// The HTTP headers the transport read, as sent (absent is `None`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Headers<'a> {
    pub protocol_version: Option<&'a str>,
    pub method: Option<&'a str>,
    pub name: Option<&'a str>,
}

/// Decides a request's era and checks what that era requires of it.
pub fn check_request(
    id: &Value,
    method: &str,
    params: &Map<String, Value>,
    headers: Headers<'_>,
) -> Result<Era, RpcError> {
    let meta = params.get("_meta").and_then(Value::as_object);
    let meta_version = meta.and_then(|m| m.get(META_PROTOCOL_VERSION));
    let Some(meta_version) = meta_version else {
        return check_legacy(id, method, headers);
    };
    let Some(version) = meta_version.as_str() else {
        return Err(invalid_params(
            id,
            format!("\"_meta\".\"{META_PROTOCOL_VERSION}\" must be a string"),
        ));
    };
    if LEGACY_VERSIONS.contains(&version) {
        // A legacy version named the modern way: serve it as legacy, which has
        // nothing more to check than the header.
        return check_legacy(id, method, headers);
    }
    if !MODERN_VERSIONS.contains(&version) {
        return Err(unsupported(id, version));
    }
    if headers.protocol_version != Some(version) {
        return Err(header_mismatch(
            id,
            PROTOCOL_VERSION_HEADER,
            headers.protocol_version,
            version,
        ));
    }
    if headers.method != Some(method) {
        return Err(header_mismatch(id, METHOD_HEADER, headers.method, method));
    }
    if method == "tools/call"
        && let Some(name) = params.get("name").and_then(Value::as_str)
    {
        let sent = headers.name.map(decode_header_value);
        if sent.as_ref().map(|s| s.as_deref()) != Some(Some(name)) {
            return Err(header_mismatch(id, NAME_HEADER, headers.name, name));
        }
    }
    if !meta
        .and_then(|m| m.get(META_CLIENT_CAPABILITIES))
        .is_some_and(Value::is_object)
    {
        return Err(invalid_params(
            id,
            format!("\"_meta\" must carry \"{META_CLIENT_CAPABILITIES}\""),
        ));
    }
    Ok(Era::Modern)
}

fn check_legacy(id: &Value, method: &str, headers: Headers<'_>) -> Result<Era, RpcError> {
    match headers.protocol_version {
        // Before 2025-06-18 there was no header; `initialize` never carries one.
        None if method == "server/discover" => Err(invalid_params(
            id,
            format!("\"_meta\" must carry \"{META_PROTOCOL_VERSION}\""),
        )),
        None => Ok(Era::Legacy),
        Some(version) if LEGACY_VERSIONS.contains(&version) => Ok(Era::Legacy),
        Some(version) if MODERN_VERSIONS.contains(&version) => Err(invalid_params(
            id,
            format!("\"_meta\" must carry \"{META_PROTOCOL_VERSION}\""),
        )),
        Some(version) => Err(unsupported(id, version)),
    }
}

/// The version a legacy `initialize` settles on: the client's when it's one we speak,
/// else our newest legacy one, which the client may refuse.
pub fn negotiate_legacy(requested: Option<&str>) -> &'static str {
    LEGACY_VERSIONS
        .iter()
        .find(|v| Some(**v) == requested)
        .copied()
        .unwrap_or(LEGACY_VERSIONS[0])
}

fn invalid_params(id: &Value, message: String) -> RpcError {
    RpcError::new(
        Some(id.clone()),
        INVALID_PARAMS,
        format!("Invalid params: {message}"),
    )
}

fn unsupported(id: &Value, requested: &str) -> RpcError {
    RpcError::new(
        Some(id.clone()),
        UNSUPPORTED_PROTOCOL_VERSION,
        "Unsupported protocol version",
    )
    .with_data(json!({"supported": supported_versions(), "requested": requested}))
}

fn header_mismatch(id: &Value, header: &str, sent: Option<&str>, body: &str) -> RpcError {
    let message = match sent {
        None => format!("Header mismatch: {header} is required"),
        Some(sent) => {
            format!(
                "Header mismatch: {header} header value {sent:?} does not match body value {body:?}"
            )
        }
    };
    RpcError::new(Some(id.clone()), HEADER_MISMATCH, message)
}

/// A header value, decoded from the `=?base64?…?=` form clients use for values that
/// aren't plain ASCII. `None` when the encoded form doesn't decode to UTF-8.
pub fn decode_header_value(raw: &str) -> Option<String> {
    match raw
        .strip_prefix("=?base64?")
        .and_then(|rest| rest.strip_suffix("?="))
    {
        Some(encoded) => BASE64
            .decode(encoded)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok()),
        None => Some(raw.to_string()),
    }
}

/// Standard alphabet, padding optional.
const BASE64: GeneralPurpose = GeneralPurpose::new(
    &base64::alphabet::STANDARD,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jsonrpc::INVALID_PARAMS;

    fn modern_params(extra: Value) -> Map<String, Value> {
        let mut params = json!({
            "_meta": {
                META_PROTOCOL_VERSION: "2026-07-28",
                META_CLIENT_CAPABILITIES: {},
            }
        })
        .as_object()
        .cloned()
        .unwrap_or_default();
        if let Value::Object(extra) = extra {
            params.extend(extra);
        }
        params
    }

    fn modern_headers<'a>(method: &'a str, name: Option<&'a str>) -> Headers<'a> {
        Headers {
            protocol_version: Some("2026-07-28"),
            method: Some(method),
            name,
        }
    }

    fn code(result: Result<Era, RpcError>) -> i64 {
        match result {
            Err(error) => error.code,
            Ok(era) => panic!("expected an error, got {era:?}"),
        }
    }

    #[test]
    fn a_request_with_meta_and_matching_headers_is_modern() {
        let params = modern_params(json!({}));
        assert_eq!(
            check_request(
                &json!(1),
                "tools/list",
                &params,
                modern_headers("tools/list", None)
            ),
            Ok(Era::Modern)
        );
    }

    #[test]
    fn a_request_without_meta_is_legacy() {
        let headers = Headers {
            protocol_version: Some("2025-11-25"),
            ..Headers::default()
        };
        assert_eq!(
            check_request(&json!(1), "tools/list", &Map::new(), headers),
            Ok(Era::Legacy)
        );
    }

    #[test]
    fn a_legacy_request_without_the_header_is_served() {
        assert_eq!(
            check_request(&json!(1), "initialize", &Map::new(), Headers::default()),
            Ok(Era::Legacy)
        );
    }

    #[test]
    fn a_legacy_request_with_an_unknown_version_header_is_unsupported() {
        let headers = Headers {
            protocol_version: Some("1999-01-01"),
            ..Headers::default()
        };
        assert_eq!(
            code(check_request(&json!(1), "tools/list", &Map::new(), headers)),
            UNSUPPORTED_PROTOCOL_VERSION
        );
    }

    #[test]
    fn a_modern_header_without_meta_is_invalid_params() {
        let headers = modern_headers("tools/list", None);
        assert_eq!(
            code(check_request(&json!(1), "tools/list", &Map::new(), headers)),
            INVALID_PARAMS
        );
    }

    #[test]
    fn discover_without_meta_is_invalid_params() {
        assert_eq!(
            code(check_request(
                &json!(1),
                "server/discover",
                &Map::new(),
                Headers::default()
            )),
            INVALID_PARAMS
        );
    }

    #[test]
    fn an_unknown_meta_version_is_unsupported_and_lists_ours() {
        let mut params = modern_params(json!({}));
        params.insert("_meta".into(), json!({META_PROTOCOL_VERSION: "2030-01-01"}));
        let error = check_request(&json!(1), "tools/list", &params, Headers::default());
        assert_eq!(
            error,
            Err(RpcError::new(
                Some(json!(1)),
                UNSUPPORTED_PROTOCOL_VERSION,
                "Unsupported protocol version"
            )
            .with_data(json!({
                "supported": ["2026-07-28", "2025-11-25", "2025-06-18", "2025-03-26"],
                "requested": "2030-01-01"
            })))
        );
    }

    #[test]
    fn the_version_header_must_match_meta() {
        let params = modern_params(json!({}));
        let mut headers = modern_headers("tools/list", None);
        headers.protocol_version = None;
        assert_eq!(
            code(check_request(&json!(1), "tools/list", &params, headers)),
            HEADER_MISMATCH
        );
        headers.protocol_version = Some("2025-11-25");
        assert_eq!(
            code(check_request(&json!(1), "tools/list", &params, headers)),
            HEADER_MISMATCH
        );
    }

    #[test]
    fn the_method_header_must_match() {
        let params = modern_params(json!({}));
        assert_eq!(
            code(check_request(
                &json!(1),
                "tools/list",
                &params,
                modern_headers("tools/call", None)
            )),
            HEADER_MISMATCH
        );
    }

    #[test]
    fn a_tool_call_needs_the_name_header() {
        let params = modern_params(json!({"name": "whoami"}));
        assert_eq!(
            code(check_request(
                &json!(1),
                "tools/call",
                &params,
                modern_headers("tools/call", None)
            )),
            HEADER_MISMATCH
        );
        assert_eq!(
            code(check_request(
                &json!(1),
                "tools/call",
                &params,
                modern_headers("tools/call", Some("call"))
            )),
            HEADER_MISMATCH
        );
        assert_eq!(
            check_request(
                &json!(1),
                "tools/call",
                &params,
                modern_headers("tools/call", Some("whoami"))
            ),
            Ok(Era::Modern)
        );
    }

    #[test]
    fn the_name_header_may_be_base64() {
        // "whoami"
        let params = modern_params(json!({"name": "whoami"}));
        assert_eq!(
            check_request(
                &json!(1),
                "tools/call",
                &params,
                modern_headers("tools/call", Some("=?base64?d2hvYW1p?="))
            ),
            Ok(Era::Modern)
        );
    }

    #[test]
    fn client_capabilities_are_required() {
        let mut params = Map::new();
        params.insert("_meta".into(), json!({META_PROTOCOL_VERSION: "2026-07-28"}));
        assert_eq!(
            code(check_request(
                &json!(1),
                "tools/list",
                &params,
                modern_headers("tools/list", None)
            )),
            INVALID_PARAMS
        );
    }

    #[test]
    fn initialize_keeps_a_version_we_speak() {
        assert_eq!(negotiate_legacy(Some("2025-06-18")), "2025-06-18");
    }

    #[test]
    fn initialize_offers_our_newest_legacy_version_otherwise() {
        assert_eq!(negotiate_legacy(Some("2024-11-05")), "2025-11-25");
        assert_eq!(negotiate_legacy(Some("2026-07-28")), "2025-11-25");
        assert_eq!(negotiate_legacy(None), "2025-11-25");
    }

    #[test]
    fn decodes_base64_header_values() {
        assert_eq!(
            decode_header_value("=?base64?SGVsbG8sIOS4lueVjA==?="),
            Some("Hello, 世界".to_string())
        );
        assert_eq!(
            decode_header_value("=?base64?IHBhZGRlZCA=?="),
            Some(" padded ".into())
        );
        assert_eq!(decode_header_value("plain"), Some("plain".into()));
    }

    #[test]
    fn broken_base64_decodes_to_nothing() {
        assert_eq!(decode_header_value("=?base64?a?="), None);
        assert_eq!(decode_header_value("=?base64?***?="), None);
    }
}
