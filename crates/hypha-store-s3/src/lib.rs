//! File storage over the S3 API (design note 0013, section 8): Cloudflare R2, AWS S3 and
//! S3-compatible servers, with one `BlobStore` adapter.
//!
//! A storage adapter, like `hypha-store-postgres`: it depends on `hypha-media`, never on
//! hosts. It opens no connections itself; HTTP goes through the `HttpClient` port, which
//! the host implements (a native client, or `fetch` on Workers), so this crate stays
//! runtime-free and builds for wasm.

mod sigv4;

use base64::Engine;
use hypha_core::{Clock, Timestamp};
use hypha_media::{BlobError, BlobHead, BlobKey, BlobStore, PresignedUpload, UploadTerms};

use crate::sigv4::{Request, Signer, encode_path};

/// One HTTP request for `HttpClient`, without a body (Hypha never sends file bytes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: &'static str,
    pub url: String,
    pub headers: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    /// The first header named `name`, ignoring case.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// Sends a request and returns whatever status came back; `Err` only when no response
/// arrived (DNS, connection, timeout). `async fn` without `Send`, like the other ports.
#[allow(async_fn_in_trait)]
pub trait HttpClient {
    async fn send(&self, request: HttpRequest) -> Result<HttpResponse, String>;
}

/// Where the bucket is and how to sign for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct S3Config {
    /// Scheme, host and port, no path: `https://<account>.r2.cloudflarestorage.com`,
    /// `https://s3.eu-central-1.amazonaws.com`, `http://localhost:9000`.
    pub endpoint: String,
    /// `auto` for R2.
    pub region: String,
    pub bucket: String,
    pub access_key_id: String,
    pub secret_access_key: String,
}

impl S3Config {
    /// `endpoint` without its scheme: what the `Host` header carries.
    fn host(&self) -> &str {
        let rest = self
            .endpoint
            .split_once("://")
            .map_or(self.endpoint.as_str(), |(_, rest)| rest);
        rest.trim_end_matches('/')
    }

    /// Path-style (`/{bucket}/{key}`), which every S3 implementation serves.
    fn path(&self, key: &BlobKey) -> String {
        encode_path(&format!("/{}/{}", self.bucket, key))
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.endpoint.trim_end_matches('/'))
    }

    fn signer(&self) -> Signer<'_> {
        Signer {
            access_key_id: &self.access_key_id,
            secret_access_key: &self.secret_access_key,
            region: &self.region,
        }
    }
}

/// A `BlobStore` on an S3 bucket. `clock` dates Hypha's own signed requests.
pub struct S3BlobStore<H, C> {
    config: S3Config,
    http: H,
    clock: C,
}

impl<H: HttpClient, C: Clock> S3BlobStore<H, C> {
    pub fn new(config: S3Config, http: H, clock: C) -> Self {
        Self {
            config,
            http,
            clock,
        }
    }

    /// Signs and sends a request without a body for `key`.
    async fn send(
        &self,
        method: &'static str,
        key: &BlobKey,
        headers: Vec<(String, String)>,
    ) -> Result<HttpResponse, BlobError> {
        let path = self.config.path(key);
        let mut all = headers.clone();
        all.extend(self.config.signer().authorize(&Request {
            method,
            host: self.config.host(),
            path: &path,
            headers: &headers,
            now: self.clock.now(),
        }));
        self.http
            .send(HttpRequest {
                method,
                url: self.config.url(&path),
                headers: all,
            })
            .await
            .map_err(|e| BlobError::Backend(format!("{method} {key}: {e}")))
    }
}

impl<H: HttpClient, C: Clock> S3BlobStore<H, C> {
    /// Creates the bucket unless it exists: for development servers and tests. Production
    /// buckets are made by their owner, with the permissions and CORS rules they need.
    pub async fn ensure_bucket(&self) -> Result<(), BlobError> {
        let path = encode_path(&format!("/{}", self.config.bucket));
        let headers = self.config.signer().authorize(&Request {
            method: "PUT",
            host: self.config.host(),
            path: &path,
            headers: &[],
            now: self.clock.now(),
        });
        let response = self
            .http
            .send(HttpRequest {
                method: "PUT",
                url: self.config.url(&path),
                headers,
            })
            .await
            .map_err(|e| BlobError::Backend(format!("create bucket: {e}")))?;
        let body = String::from_utf8_lossy(&response.body);
        match response.status {
            200 => Ok(()),
            409 if body.contains("BucketAlreadyOwnedByYou")
                || body.contains("BucketAlreadyExists") =>
            {
                Ok(())
            }
            status => Err(BlobError::Backend(format!(
                "create bucket {}: HTTP {status}: {}",
                self.config.bucket,
                body.chars().take(300).collect::<String>()
            ))),
        }
    }
}

/// An unexpected answer, with the start of S3's error document for the logs.
fn unexpected(what: &str, key: &BlobKey, response: &HttpResponse) -> BlobError {
    let body = String::from_utf8_lossy(&response.body);
    let body: String = body.chars().take(300).collect();
    BlobError::Backend(format!("{what} {key}: HTTP {}: {body}", response.status))
}

impl<H: HttpClient, C: Clock> BlobStore for S3BlobStore<H, C> {
    async fn presign_upload(
        &self,
        key: &BlobKey,
        terms: &UploadTerms,
        now: Timestamp,
    ) -> Result<PresignedUpload, BlobError> {
        // What the client sends; `content-length` is signed too, but clients set it from
        // the body themselves, so it isn't in the list they're given.
        let mut headers = vec![
            ("content-type".to_string(), terms.content_type.to_string()),
            ("cache-control".to_string(), terms.cache_control.to_string()),
        ];
        if let Some(checksum) = terms.checksum {
            headers.push((
                "x-amz-checksum-sha256".to_string(),
                base64::engine::general_purpose::STANDARD.encode(checksum.as_bytes()),
            ));
        }
        let mut signed = headers.clone();
        signed.push((
            "content-length".to_string(),
            terms.content_length.to_string(),
        ));
        let path = self.config.path(key);
        let query = self.config.signer().presign(
            &Request {
                method: "PUT",
                host: self.config.host(),
                path: &path,
                headers: &signed,
                now,
            },
            terms.expires_in_secs,
        );
        Ok(PresignedUpload {
            url: format!("{}?{query}", self.config.url(&path)),
            method: "PUT",
            headers,
            expires_at: now.plus_secs(i64::from(terms.expires_in_secs)),
        })
    }

    async fn head(&self, key: &BlobKey) -> Result<Option<BlobHead>, BlobError> {
        let response = self.send("HEAD", key, vec![]).await?;
        match response.status {
            200 => {
                let size = response
                    .header("content-length")
                    .and_then(|v| v.parse().ok())
                    .ok_or_else(|| unexpected("HEAD (no content-length)", key, &response))?;
                Ok(Some(BlobHead { size }))
            }
            404 => Ok(None),
            _ => Err(unexpected("HEAD", key, &response)),
        }
    }

    async fn read_range(&self, key: &BlobKey, start: u64, len: u64) -> Result<Vec<u8>, BlobError> {
        if len == 0 {
            return Ok(Vec::new());
        }
        let last = start.saturating_add(len - 1);
        let range = vec![("range".to_string(), format!("bytes={start}-{last}"))];
        let response = self.send("GET", key, range).await?;
        match response.status {
            // 200 when the server ignores the range: cut it here.
            200 => Ok(response
                .body
                .into_iter()
                .skip(usize::try_from(start).unwrap_or(usize::MAX))
                .take(usize::try_from(len).unwrap_or(usize::MAX))
                .collect()),
            206 => Ok(response.body),
            // The range starts past the end.
            416 => Ok(Vec::new()),
            _ => Err(unexpected("GET", key, &response)),
        }
    }

    async fn delete(&self, key: &BlobKey) -> Result<(), BlobError> {
        let response = self.send("DELETE", key, vec![]).await?;
        match response.status {
            200 | 204 | 404 => Ok(()),
            _ => Err(unexpected("DELETE", key, &response)),
        }
    }

    async fn presign_download(
        &self,
        key: &BlobKey,
        now: Timestamp,
        expires_in_secs: u32,
    ) -> Result<String, BlobError> {
        let path = self.config.path(key);
        let query = self.config.signer().presign(
            &Request {
                method: "GET",
                host: self.config.host(),
                path: &path,
                headers: &[],
                now,
            },
            expires_in_secs,
        );
        Ok(format!("{}?{query}", self.config.url(&path)))
    }
}

/// `HttpClient` on `reqwest`, for native hosts and tests.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Default)]
pub struct ReqwestClient(reqwest::Client);

#[cfg(not(target_arch = "wasm32"))]
impl ReqwestClient {
    pub fn new(client: reqwest::Client) -> Self {
        Self(client)
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl HttpClient for ReqwestClient {
    async fn send(&self, request: HttpRequest) -> Result<HttpResponse, String> {
        let method =
            reqwest::Method::from_bytes(request.method.as_bytes()).map_err(|e| e.to_string())?;
        let mut builder = self.0.request(method, &request.url);
        for (name, value) in &request.headers {
            builder = builder.header(name, value);
        }
        let response = builder.send().await.map_err(|e| e.to_string())?;
        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .filter_map(|(n, v)| Some((n.to_string(), v.to_str().ok()?.to_string())))
            .collect();
        let body = response.bytes().await.map_err(|e| e.to_string())?.to_vec();
        Ok(HttpResponse {
            status,
            headers,
            body,
        })
    }
}
