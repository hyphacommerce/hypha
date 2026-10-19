//! What the host hands a spore when it builds it (design note 0030, §4 and §8): HTTP,
//! time, IDs, randomness, the `SecretBox`, and the spore's own section of
//! `hypha.config.toml` (`[spores.<name>]`), which it parses itself.
//!
//! Everything is a shared, `Send + Sync` capability, so a spore can keep its `SporeEnv`
//! (or clones of it) for as long as it lives, and nothing here is tied to a runtime: the
//! native host gives Tokio's and the operating system's, a Workers host the runtime's
//! (`fetch`, `Date.now()`), and tests their own (SP8).

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use hypha_apps::{HasSporeSettings, read_spore_settings};
use hypha_core::{AppKey, Clock, IdGenerator, Random, SealError, Sealed, SecretBox, Timestamp};
use serde::de::DeserializeOwned;
use thiserror::Error;
use uuid::Uuid;

/// A boxed future that can move between threads, as the host's capabilities return.
pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

/// The methods a spore calls with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

impl HttpMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            HttpMethod::Get => "GET",
            HttpMethod::Post => "POST",
            HttpMethod::Put => "PUT",
            HttpMethod::Patch => "PATCH",
            HttpMethod::Delete => "DELETE",
        }
    }
}

/// One request a spore sends (to Stripe's API, say).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: HttpMethod,
    pub url: String,
    /// Every header, the content type included.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// How long to wait for the answer.
    pub timeout: Duration,
    /// The most of the answer's body read; more is `HttpError::TooLarge`.
    pub max_answer: usize,
}

impl HttpRequest {
    /// The default `timeout`.
    pub const TIMEOUT: Duration = Duration::from_secs(10);
    /// The default `max_answer`: 1 MiB.
    pub const MAX_ANSWER: usize = 1 << 20;

    /// A request with no headers or body, and the default timeout and answer cap.
    pub fn new(method: HttpMethod, url: impl Into<String>) -> Self {
        Self {
            method,
            url: url.into(),
            headers: Vec::new(),
            body: Vec::new(),
            timeout: Self::TIMEOUT,
            max_answer: Self::MAX_ANSWER,
        }
    }

    /// The request with one more header.
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// The request with `body`.
    pub fn body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = body.into();
        self
    }
}

/// The answer, whatever its status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub content_type: Option<String>,
    pub body: Vec<u8>,
}

/// Why no answer came.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum HttpError {
    #[error("no answer in time")]
    Timeout,
    #[error("the answer was larger than allowed")]
    TooLarge,
    /// Refused before sending (an address that isn't public, unless the operator allows
    /// private ones: `outbound.allow_private`), or failed on the way; the host logs the
    /// details.
    #[error("{0}")]
    Failed(String),
}

/// Sends a request (`SporeEnv` is one). `async fn` without `Send`, like every port.
#[allow(async_fn_in_trait)]
pub trait HttpClient {
    async fn send(&self, request: HttpRequest) -> Result<HttpResponse, HttpError>;
}

/// How the host sends a request.
type SendRequest = dyn Fn(HttpRequest) -> BoxFuture<Result<HttpResponse, HttpError>> + Send + Sync;

/// The host's HTTP client: the one calls to apps go through (natively `reqwest` with
/// the resolver that keeps to public addresses, `fetch` on Workers), with no redirects
/// followed and the answer capped.
#[derive(Clone)]
pub struct SporeHttp(Arc<SendRequest>);

impl SporeHttp {
    pub fn new(
        send: impl Fn(HttpRequest) -> BoxFuture<Result<HttpResponse, HttpError>> + Send + Sync + 'static,
    ) -> Self {
        Self(Arc::new(send))
    }
}

impl HttpClient for SporeHttp {
    async fn send(&self, request: HttpRequest) -> Result<HttpResponse, HttpError> {
        (self.0)(request).await
    }
}

impl fmt::Debug for SporeHttp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SporeHttp")
    }
}

/// What every spore of a host shares: the host's capabilities. The host builds one at
/// startup and `SporeSet::build` gives each spore its `SporeEnv` from it.
#[derive(Clone)]
pub struct SporeHost {
    pub clock: Arc<dyn Clock + Send + Sync>,
    pub ids: Arc<dyn IdGenerator + Send + Sync>,
    pub random: Arc<dyn Random + Send + Sync>,
    /// Seals with `HYPHA_SECRET_KEY`, as 2FA secrets and app secrets are.
    pub secrets: Arc<dyn SecretBox + Send + Sync>,
    pub http: SporeHttp,
}

impl SporeHost {
    /// The environment of the spore `name`, with its configuration section (empty when
    /// `hypha.config.toml` has none).
    pub fn env(&self, name: &'static str, config: toml::Table) -> SporeEnv {
        SporeEnv {
            name,
            host: self.clone(),
            config,
        }
    }
}

/// One spore's environment (`Spore::new`). It is the spore's clock, ID generator,
/// randomness, `SecretBox` and HTTP client itself (it implements those ports), so a
/// spore passes `&env` wherever a command asks for one.
#[derive(Clone)]
pub struct SporeEnv {
    name: &'static str,
    host: SporeHost,
    config: toml::Table,
}

impl SporeEnv {
    /// The spore's name (`stripe`).
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// Its section of `hypha.config.toml`, `[spores.<name>]`, as written: operator
    /// settings (an API base for tests, a timeout), never the merchant's (those are its
    /// settings, design note 0030, §8) and never secrets.
    pub fn config(&self) -> &toml::Table {
        &self.config
    }

    /// Its section read into `T` (`#[serde(deny_unknown_fields)]` recommended, so a
    /// typo is an error rather than a default).
    pub fn config_as<T: DeserializeOwned>(&self) -> Result<T, SporeError> {
        toml::Value::Table(self.config.clone())
            .try_into()
            .map_err(|e: toml::de::Error| SporeError::Config {
                key: None,
                message: e.message().to_string(),
            })
    }
}

impl SporeEnv {
    /// Its settings as the merchant saved them (design note 0030, §8), secret fields
    /// opened, read into `T` through `ports` (any with `HasSporeSettings`: a request's,
    /// a handler's). `None` before they're first saved.
    pub async fn settings<T: DeserializeOwned, P: HasSporeSettings + ?Sized>(
        &self,
        ports: &P,
    ) -> Result<Option<T>, SettingsError> {
        let key = AppKey::of_spore(self.name).map_err(|e| SettingsError::Read(e.to_string()))?;
        let values = read_spore_settings(ports, self, &key)
            .await
            .map_err(|e| SettingsError::Read(e.to_string()))?;
        values
            .map(|v| serde_json::from_value(serde_json::Value::Object(v)))
            .transpose()
            .map_err(|e| SettingsError::Shape(e.to_string()))
    }
}

/// Why a spore couldn't read its settings.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SettingsError {
    /// Storage failed, or a secret didn't open (another `HYPHA_SECRET_KEY`).
    #[error("cannot read the settings: {0}")]
    Read(String),
    /// They don't fit the spore's type (its schema changed since they were saved).
    #[error("the settings don't fit: {0}")]
    Shape(String),
}

impl fmt::Debug for SporeEnv {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SporeEnv")
            .field("name", &self.name)
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl Clock for SporeEnv {
    fn now(&self) -> Timestamp {
        self.host.clock.now()
    }
}

impl IdGenerator for SporeEnv {
    fn new_uuid(&self) -> Uuid {
        self.host.ids.new_uuid()
    }
}

impl Random for SporeEnv {
    fn fill(&self, bytes: &mut [u8]) {
        self.host.random.fill(bytes);
    }
}

/// The host's `SecretBox` (`HYPHA_SECRET_KEY`), for what a spore seals itself; its
/// settings' secret fields are sealed and opened by core (§8).
impl SecretBox for SporeEnv {
    fn seal(&self, plaintext: &[u8], context: &[u8], nonce: [u8; 24]) -> Sealed {
        self.host.secrets.seal(plaintext, context, nonce)
    }

    fn open(&self, sealed: &Sealed, context: &[u8]) -> Result<Vec<u8>, SealError> {
        self.host.secrets.open(sealed, context)
    }
}

impl HttpClient for SporeEnv {
    async fn send(&self, request: HttpRequest) -> Result<HttpResponse, HttpError> {
        self.host.http.send(request).await
    }
}

/// Why a spore couldn't be built (`Spore::new`): the host stops starting and names the
/// spore with this.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SporeError {
    /// Its `[spores.<name>]` section is wrong: `key` (under the section) when one key is
    /// to blame.
    #[error("{}", config_message(key.as_deref(), message))]
    Config {
        key: Option<String>,
        message: String,
    },
    /// Anything else that keeps it from starting, in words for the operator.
    #[error("{0}")]
    Other(String),
}

fn config_message(key: Option<&str>, message: &str) -> String {
    match key {
        Some(key) => format!("{key}: {message}"),
        None => message.to_string(),
    }
}

impl hypha_core::ErrorCode for SporeError {
    fn code(&self) -> &'static str {
        match self {
            SporeError::Config { .. } => "spore_config_invalid",
            SporeError::Other(_) => "spore_failed",
        }
    }
}

/// A spore that couldn't be built, by name: what the host prints before it exits.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("spore {spore:?}: {error}")]
pub struct SporeStartError {
    pub spore: &'static str,
    pub error: SporeError,
}

/// A host for tests: a fixed clock and IDs, sevens for randomness, a `SecretBox` that
/// only checks the context, and HTTP that answers 204 to everything.
#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use super::*;

    struct Epoch;
    impl Clock for Epoch {
        fn now(&self) -> Timestamp {
            Timestamp::UNIX_EPOCH
        }
    }
    struct Nil;
    impl IdGenerator for Nil {
        fn new_uuid(&self) -> Uuid {
            Uuid::nil()
        }
    }
    struct Sevens;
    impl Random for Sevens {
        fn fill(&self, bytes: &mut [u8]) {
            bytes.fill(7);
        }
    }
    /// "Seals" by prefixing the context: no secrecy, only the context check.
    struct Plain;
    impl SecretBox for Plain {
        fn seal(&self, plaintext: &[u8], context: &[u8], _: [u8; 24]) -> Sealed {
            let mut bytes = context.to_vec();
            bytes.push(0);
            bytes.extend_from_slice(plaintext);
            Sealed::from_bytes(bytes)
        }
        fn open(&self, sealed: &Sealed, context: &[u8]) -> Result<Vec<u8>, SealError> {
            match sealed.as_bytes().strip_prefix(context) {
                Some([0, rest @ ..]) => Ok(rest.to_vec()),
                _ => Err(SealError("sealed for another context")),
            }
        }
    }

    /// The test host.
    pub fn host() -> SporeHost {
        SporeHost {
            clock: Arc::new(Epoch),
            ids: Arc::new(Nil),
            random: Arc::new(Sevens),
            secrets: Arc::new(Plain),
            http: SporeHttp::new(|_| {
                Box::pin(async {
                    Ok(HttpResponse {
                        status: 204,
                        content_type: None,
                        body: Vec::new(),
                    })
                })
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, serde::Deserialize, PartialEq)]
    #[serde(deny_unknown_fields)]
    struct Section {
        api_base: String,
        #[serde(default)]
        timeout_ms: Option<u32>,
    }

    fn table(toml: &str) -> toml::Table {
        toml.parse().unwrap()
    }

    #[test]
    fn a_section_is_read_into_the_spores_own_type() {
        let env = testing::host().env("pay", table("api_base = \"http://localhost:9\""));
        assert_eq!(env.name(), "pay");
        assert_eq!(
            env.config_as::<Section>(),
            Ok(Section {
                api_base: "http://localhost:9".to_string(),
                timeout_ms: None,
            })
        );
    }

    #[test]
    fn an_unknown_key_in_a_section_is_a_config_error() {
        let env = testing::host().env("pay", table("api_base = \"x\"\napi_bsae = \"y\""));
        let error = env.config_as::<Section>().unwrap_err();
        assert!(
            matches!(&error, SporeError::Config { key: None, message } if message.contains("api_bsae")),
            "{error:?}"
        );
    }

    #[test]
    fn the_env_is_the_hosts_clock_ids_randomness_secrets_and_http() {
        let env = testing::host().env("pay", toml::Table::new());
        assert_eq!(env.now(), Timestamp::UNIX_EPOCH);
        assert_eq!(env.new_uuid(), Uuid::nil());
        let mut bytes = [0; 3];
        env.fill(&mut bytes);
        assert_eq!(bytes, [7; 3]);
        let sealed = env.seal(b"sk_test", b"ctx", [0; 24]);
        assert_eq!(env.open(&sealed, b"ctx"), Ok(b"sk_test".to_vec()));
        assert!(env.open(&sealed, b"other").is_err());
        let request = HttpRequest::new(HttpMethod::Post, "https://example.com")
            .header("content-type", "application/x-www-form-urlencoded")
            .body("a=1");
        let answer = pollster::block_on(env.send(request));
        assert_eq!(answer.map(|a| a.status), Ok(204));
    }

    #[test]
    fn errors_name_the_spore_and_the_key_when_there_is_one() {
        let error = SporeStartError {
            spore: "pay",
            error: SporeError::Config {
                key: Some("timeout_ms".to_string()),
                message: "at most 30000".to_string(),
            },
        };
        assert_eq!(
            error.to_string(),
            "spore \"pay\": timeout_ms: at most 30000"
        );
    }
}
