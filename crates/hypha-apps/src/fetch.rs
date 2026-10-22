//! Fetching an app's manifest from its URL (the URL is the app's OAuth client ID). The
//! fetching is a host port, `ManifestFetcher`: natively and on Workers it's a GET through
//! the outbound client, which reaches only public addresses unless
//! `outbound.allow_private`.

use hypha_core::{CallbackUrl, ErrorCode};
use thiserror::Error;

use crate::manifest::{AppManifest, MAX_MANIFEST_BYTES, ManifestError};

/// Reads a manifest's bytes from its URL. `Err` says what happened (a status, a
/// timeout), for the caller; never how the network looked.
#[allow(async_fn_in_trait)]
pub trait ManifestFetcher {
    async fn fetch(&self, url: &str) -> Result<Vec<u8>, String>;
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FetchError {
    /// Not an https URL (or http to localhost), or nothing came back.
    #[error("the app manifest at {url:?} can't be read: {reason}")]
    Unreachable { url: String, reason: String },
    #[error(transparent)]
    Invalid(#[from] ManifestError),
}

impl ErrorCode for FetchError {
    fn code(&self) -> &'static str {
        match self {
            FetchError::Unreachable { .. } => "manifest_unreachable",
            FetchError::Invalid(e) => e.code(),
        }
    }
}

/// The manifest at `url`, checked: at most `MAX_MANIFEST_BYTES` of JSON.
pub async fn fetch_manifest(
    fetcher: &impl ManifestFetcher,
    url: &str,
) -> Result<AppManifest, FetchError> {
    let unreachable = |reason: String| FetchError::Unreachable {
        url: url.to_string(),
        reason,
    };
    CallbackUrl::new(url)
        .map_err(|_| unreachable("not an https URL (or http to localhost)".to_string()))?;
    let bytes = fetcher.fetch(url).await.map_err(unreachable)?;
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err(unreachable(format!(
            "larger than {MAX_MANIFEST_BYTES} bytes"
        )));
    }
    Ok(AppManifest::parse(&bytes)?)
}
