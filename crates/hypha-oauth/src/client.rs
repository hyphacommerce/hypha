//! Who OAuth's codes and tokens belong to, and the clients that ask for them.
//!
//! Two kinds of client use the same flow (codes, PKCE, rotating refresh tokens):
//!
//! - **Apps**, whose client ID is their manifest's URL (`hypha-app.json`): installing one
//!   gives it a grant and a signing secret.
//! - **MCP clients** (Claude, ChatGPT, Cursor, a merchant's own), whose client ID is the
//!   URL of an OAuth Client ID Metadata Document: connecting one makes a connected agent
//!   that acts for the staff member who approved it, and its tokens work at `/mcp` only.

use hypha_apps::{AppManifest, ManifestFetcher};
use hypha_core::{AgentId, AppKey, ErrorCode};
use serde_json::{Map, Value, json};
use thiserror::Error;
use url::{Host, Url};

/// Who a code or a token pair was issued to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OAuthClient {
    App(AppKey),
    /// A connected agent: its tokens are good at `/mcp` and nowhere else.
    Agent(AgentId),
}

impl OAuthClient {
    pub fn app(&self) -> Option<&AppKey> {
        match self {
            OAuthClient::App(key) => Some(key),
            OAuthClient::Agent(_) => None,
        }
    }

    pub fn agent(&self) -> Option<AgentId> {
        match self {
            OAuthClient::Agent(id) => Some(*id),
            OAuthClient::App(_) => None,
        }
    }

    /// From storage's two columns, exactly one of them set.
    pub fn from_parts(app: Option<String>, agent: Option<AgentId>) -> Result<Self, String> {
        match (app, agent) {
            (Some(key), None) => AppKey::new(&key)
                .map(OAuthClient::App)
                .map_err(|e| e.to_string()),
            (None, Some(agent)) => Ok(OAuthClient::Agent(agent)),
            _ => Err("an OAuth record belongs to exactly one app or agent".to_string()),
        }
    }
}

/// What a pending authorization request is for.
#[derive(Debug, Clone, PartialEq)]
pub enum RequestClient {
    /// Installing (or updating) an app from this manifest.
    App(Box<AppManifest>),
    /// Connecting an MCP client, as its metadata document describes it.
    Agent(ClientMetadata),
}

/// The most a client metadata document may weigh. The draft recommends 5 KB; a document
/// is a handful of fields.
pub const MAX_CLIENT_METADATA_BYTES: usize = 5 * 1024;
/// The longest `client_name` kept.
pub const MAX_CLIENT_NAME_LEN: usize = 200;
/// The most redirect URIs a document may list.
pub const MAX_REDIRECT_URIS: usize = 20;

/// An MCP client as its OAuth Client ID Metadata Document describes it: the document
/// lives at the client ID's URL, so whoever controls that URL is the client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientMetadata {
    client_id: String,
    client_name: String,
    redirect_uris: Vec<String>,
    logo_uri: Option<String>,
    client_uri: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ClientMetadataError {
    /// The client ID isn't a URL a document may live at, or nothing came back from it.
    #[error("the client metadata document at {url:?} can't be read: {reason}")]
    Unreachable { url: String, reason: String },
    /// A document came back, but it isn't a valid one for this client ID.
    #[error("the client metadata document at {url:?} is invalid: {reason}")]
    Invalid { url: String, reason: String },
}

impl ErrorCode for ClientMetadataError {
    fn code(&self) -> &'static str {
        match self {
            ClientMetadataError::Unreachable { .. } => "client_metadata_unreachable",
            ClientMetadataError::Invalid { .. } => "client_metadata_invalid",
        }
    }
}

/// Whether `url` is http(s) to this machine: a native client's loopback redirect, or a
/// document served in development.
fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(ip)) => ip.is_loopback(),
        Some(Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    }
}

/// A client ID that may name a metadata document: an `https` URL with a path, no
/// fragment, no credentials and no dot segments. `http` only to a loopback host, for
/// development (the outbound client refuses private addresses unless allowed anyway).
pub fn check_client_id(client_id: &str) -> Result<Url, String> {
    let url = Url::parse(client_id).map_err(|_| "not a URL".to_string())?;
    match url.scheme() {
        "https" => {}
        "http" if is_loopback(&url) => {}
        _ => return Err("not an https URL".to_string()),
    }
    if url.fragment().is_some() {
        return Err("a client ID has no fragment".to_string());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("a client ID carries no credentials".to_string());
    }
    if url.path() == "/" || url.path().is_empty() {
        return Err("a client ID has a path (the document's)".to_string());
    }
    // `Url` resolves dot segments; a client ID that had any reads differently now.
    if url.as_str() != client_id {
        return Err("a client ID is a normalized URL (no dot segments)".to_string());
    }
    Ok(url)
}

/// Whether `uri` may receive codes: `https`, or `http` to a loopback host (a native
/// client listening on this machine), without a fragment.
fn check_redirect_uri(uri: &str) -> Result<Url, String> {
    let url = Url::parse(uri).map_err(|_| format!("redirect URI {uri:?} is not a URL"))?;
    let ok = match url.scheme() {
        "https" => true,
        "http" => is_loopback(&url),
        _ => false,
    };
    if !ok {
        return Err(format!(
            "redirect URI {uri:?} is neither https nor http to a loopback address"
        ));
    }
    if url.fragment().is_some() {
        return Err(format!("redirect URI {uri:?} has a fragment"));
    }
    Ok(url)
}

/// An optional link the consent page shows (`logo_uri`, `client_uri`): `https` only, so
/// the admin never loads or links anything else.
fn check_link(field: &str, value: Option<&Value>) -> Result<Option<String>, String> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(link)) => match Url::parse(link) {
            Ok(url) if url.scheme() == "https" => Ok(Some(link.clone())),
            _ => Err(format!("{field} is not an https URL")),
        },
        Some(_) => Err(format!("{field} is not a string")),
    }
}

impl ClientMetadata {
    /// The document served at `client_id`, checked: JSON naming `client_id` exactly, a
    /// `client_name`, at least one redirect URI (each `https` or loopback `http`), a public
    /// client (`token_endpoint_auth_method` `none` if given, no secret), and the
    /// authorization code flow if it lists grant or response types.
    pub fn parse(client_id: &str, bytes: &[u8]) -> Result<Self, ClientMetadataError> {
        let invalid = |reason: String| ClientMetadataError::Invalid {
            url: client_id.to_string(),
            reason,
        };
        if bytes.len() > MAX_CLIENT_METADATA_BYTES {
            return Err(invalid(format!(
                "larger than {MAX_CLIENT_METADATA_BYTES} bytes"
            )));
        }
        let document: Map<String, Value> = match serde_json::from_slice(bytes) {
            Ok(Value::Object(fields)) => fields,
            _ => return Err(invalid("not a JSON object".to_string())),
        };
        Self::from_fields(client_id, &document).map_err(invalid)
    }

    fn from_fields(client_id: &str, document: &Map<String, Value>) -> Result<Self, String> {
        match document.get("client_id") {
            Some(Value::String(named)) if named == client_id => {}
            Some(Value::String(named)) => {
                return Err(format!(
                    "its client_id {named:?} isn't the URL it was read from"
                ));
            }
            _ => return Err("client_id is required".to_string()),
        }
        let client_name = match document.get("client_name") {
            Some(Value::String(name)) if !name.trim().is_empty() => name.trim().to_string(),
            _ => return Err("client_name is required".to_string()),
        };
        if client_name.chars().count() > MAX_CLIENT_NAME_LEN {
            return Err(format!(
                "client_name is longer than {MAX_CLIENT_NAME_LEN} characters"
            ));
        }
        let redirect_uris = match document.get("redirect_uris") {
            Some(Value::Array(uris)) if !uris.is_empty() => uris
                .iter()
                .map(|u| match u {
                    Value::String(uri) => check_redirect_uri(uri).map(|_| uri.clone()),
                    _ => Err("redirect_uris holds strings".to_string()),
                })
                .collect::<Result<Vec<_>, _>>()?,
            _ => return Err("redirect_uris is required: at least one".to_string()),
        };
        if redirect_uris.len() > MAX_REDIRECT_URIS {
            return Err(format!("redirect_uris lists more than {MAX_REDIRECT_URIS}"));
        }
        // A metadata document is public: a secret in it would be no secret, and the
        // document must not claim one.
        for secret in ["client_secret", "client_secret_expires_at"] {
            if document.contains_key(secret) {
                return Err(format!("a client metadata document has no {secret}"));
            }
        }
        match document.get("token_endpoint_auth_method") {
            None | Some(Value::Null) => {}
            Some(Value::String(method)) if method == "none" => {}
            Some(other) => {
                return Err(format!(
                    "token_endpoint_auth_method {other} isn't supported: only public clients \
                     (none) with PKCE"
                ));
            }
        }
        let lists = |field: &str, wanted: &str| match document.get(field) {
            None | Some(Value::Null) => Ok(()),
            Some(Value::Array(items)) if items.iter().any(|i| i == wanted) => Ok(()),
            Some(_) => Err(format!("{field} must include {wanted:?}")),
        };
        lists("grant_types", "authorization_code")?;
        lists("response_types", "code")?;
        Ok(Self {
            client_id: client_id.to_string(),
            client_name,
            redirect_uris,
            logo_uri: check_link("logo_uri", document.get("logo_uri"))?,
            client_uri: check_link("client_uri", document.get("client_uri"))?,
        })
    }

    /// Whether codes may go to `uri`: one of the document's redirect URIs exactly, or, for
    /// a loopback `http` one, the same but for its port (a native client listens wherever
    /// the system lets it, RFC 8252 §7.3).
    pub fn allows_redirect(&self, uri: &str) -> bool {
        if self.redirect_uris.iter().any(|r| r == uri) {
            return true;
        }
        let Ok(asked) = check_redirect_uri(uri) else {
            return false;
        };
        if asked.scheme() != "http" || !is_loopback(&asked) {
            return false;
        }
        let without_port = |url: &Url| {
            let mut url = url.clone();
            // Only fails for URLs that can't have a port, which loopback http ones can.
            let _ = url.set_port(None);
            url
        };
        let asked = without_port(&asked);
        self.redirect_uris.iter().any(|r| {
            Url::parse(r)
                .ok()
                .filter(|r| r.scheme() == "http" && is_loopback(r))
                .is_some_and(|r| without_port(&r) == asked)
        })
    }

    pub fn client_id(&self) -> &str {
        &self.client_id
    }
    pub fn client_name(&self) -> &str {
        &self.client_name
    }
    pub fn redirect_uris(&self) -> &[String] {
        &self.redirect_uris
    }
    pub fn logo_uri(&self) -> Option<&str> {
        self.logo_uri.as_deref()
    }
    pub fn client_uri(&self) -> Option<&str> {
        self.client_uri.as_deref()
    }

    /// Whether every redirect URI is on this machine: anyone can run a client there and
    /// call it by this name, so the consent page warns.
    pub fn loopback_only(&self) -> bool {
        self.redirect_uris
            .iter()
            .all(|r| Url::parse(r).is_ok_and(|u| is_loopback(&u)))
    }

    /// As stored: the fields kept, as the document named them.
    pub fn to_value(&self) -> Value {
        json!({
            "client_id": self.client_id,
            "client_name": self.client_name,
            "redirect_uris": self.redirect_uris,
            "logo_uri": self.logo_uri,
            "client_uri": self.client_uri,
        })
    }

    /// From storage, with the rules checked again.
    pub fn from_value(client_id: &str, value: &Value) -> Result<Self, String> {
        let Value::Object(fields) = value else {
            return Err("stored client metadata is not an object".to_string());
        };
        Self::from_fields(client_id, fields)
    }
}

/// The document at `client_id`, fetched and checked. Read when a person connects a
/// client, never cached: connecting is rare (and rate limited), and a client that
/// changed its redirect URIs is believed at once.
pub async fn fetch_client_metadata(
    fetcher: &impl ManifestFetcher,
    client_id: &str,
) -> Result<ClientMetadata, ClientMetadataError> {
    let unreachable = |reason: String| ClientMetadataError::Unreachable {
        url: client_id.to_string(),
        reason,
    };
    check_client_id(client_id).map_err(unreachable)?;
    let bytes = fetcher.fetch(client_id).await.map_err(unreachable)?;
    ClientMetadata::parse(client_id, &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "https://client.example/oauth/metadata.json";

    fn document(extra: Value) -> Vec<u8> {
        let mut base = json!({
            "client_id": ID,
            "client_name": "Example client",
            "redirect_uris": ["https://client.example/callback", "http://127.0.0.1:3000/cb"],
        });
        if let (Value::Object(base), Value::Object(extra)) = (&mut base, extra) {
            base.extend(extra);
        }
        base.to_string().into_bytes()
    }

    fn invalid(bytes: &[u8]) -> String {
        match ClientMetadata::parse(ID, bytes) {
            Err(ClientMetadataError::Invalid { reason, .. }) => reason,
            other => panic!("expected invalid, got {other:?}"),
        }
    }

    #[test]
    fn a_document_names_its_own_url() {
        let parsed = ClientMetadata::parse(ID, &document(json!({}))).unwrap();
        assert_eq!(parsed.client_name(), "Example client");
        let other = document(json!({"client_id": "https://evil.example/metadata.json"}));
        assert!(invalid(&other).contains("isn't the URL it was read from"));
        let trailing = document(json!({"client_id": format!("{ID}/")}));
        assert!(invalid(&trailing).contains("isn't the URL"));
    }

    #[test]
    fn a_document_needs_a_name_and_redirect_uris() {
        assert!(invalid(&document(json!({"client_name": " "}))).contains("client_name"));
        assert!(invalid(&document(json!({"redirect_uris": []}))).contains("redirect_uris"));
        assert!(invalid(b"[]").contains("not a JSON object"));
        assert!(invalid(b"{").contains("not a JSON object"));
        let big = vec![b' '; MAX_CLIENT_METADATA_BYTES + 1];
        assert!(invalid(&big).contains("larger than"));
    }

    #[test]
    fn redirect_uris_are_https_or_loopback_http() {
        for bad in [
            "http://client.example/cb",
            "cursor://callback",
            "https://client.example/cb#frag",
            "not a url",
        ] {
            let doc = document(json!({"redirect_uris": [bad]}));
            assert!(invalid(&doc).contains("redirect URI"), "{bad}");
        }
        for good in [
            "http://localhost/cb",
            "http://[::1]:9/cb",
            "http://127.0.0.1/cb",
        ] {
            let doc = document(json!({"redirect_uris": [good]}));
            assert!(ClientMetadata::parse(ID, &doc).is_ok(), "{good}");
        }
    }

    #[test]
    fn only_public_clients_connect() {
        let doc = document(json!({"token_endpoint_auth_method": "private_key_jwt"}));
        assert!(invalid(&doc).contains("only public clients"));
        let doc = document(json!({"client_secret": "x"}));
        assert!(invalid(&doc).contains("client_secret"));
        let doc = document(json!({"token_endpoint_auth_method": "none"}));
        assert!(ClientMetadata::parse(ID, &doc).is_ok());
        let doc = document(json!({"grant_types": ["client_credentials"]}));
        assert!(invalid(&doc).contains("authorization_code"));
        let doc = document(json!({"response_types": ["token"]}));
        assert!(invalid(&doc).contains("\"code\""));
    }

    #[test]
    fn links_shown_to_staff_are_https() {
        let doc = document(json!({"logo_uri": "http://client.example/logo.png"}));
        assert!(invalid(&doc).contains("logo_uri"));
        let doc = document(json!({"logo_uri": "https://client.example/logo.png"}));
        let parsed = ClientMetadata::parse(ID, &doc).unwrap();
        assert_eq!(parsed.logo_uri(), Some("https://client.example/logo.png"));
    }

    #[test]
    fn redirects_match_exactly_but_a_loopback_port() {
        let parsed = ClientMetadata::parse(ID, &document(json!({}))).unwrap();
        assert!(parsed.allows_redirect("https://client.example/callback"));
        assert!(!parsed.allows_redirect("https://client.example/callback/"));
        assert!(!parsed.allows_redirect("https://client.example:444/callback"));
        assert!(parsed.allows_redirect("http://127.0.0.1:3000/cb"));
        assert!(parsed.allows_redirect("http://127.0.0.1:51234/cb"));
        assert!(!parsed.allows_redirect("http://127.0.0.1:51234/other"));
        assert!(
            !parsed.allows_redirect("http://localhost:3000/cb"),
            "another host"
        );
        assert!(!parsed.loopback_only());
    }

    #[test]
    fn client_ids_are_https_urls_with_a_path() {
        assert!(check_client_id(ID).is_ok());
        assert!(check_client_id("http://127.0.0.1:8080/client.json").is_ok());
        for bad in [
            "http://client.example/metadata.json",
            "https://client.example",
            "https://client.example/",
            "https://user:pw@client.example/m.json",
            "https://client.example/m.json#x",
            "https://client.example/a/../m.json",
            "client.example/m.json",
        ] {
            assert!(check_client_id(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn stored_metadata_reads_back() {
        let parsed = ClientMetadata::parse(ID, &document(json!({}))).unwrap();
        assert_eq!(
            ClientMetadata::from_value(ID, &parsed.to_value()),
            Ok(parsed)
        );
    }
}
