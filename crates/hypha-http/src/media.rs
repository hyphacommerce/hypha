//! Media in the host: the file store (`[media.s3]`, with the keys in
//! `HYPHA_S3_ACCESS_KEY_ID` and `HYPHA_S3_SECRET_ACCESS_KEY`) and how asset URLs are built
//! (`media.public_url`, `media.transforms`).

use hypha_core::{Clock, Timestamp};
use hypha_media::{Asset, AssetKind, AssetStatus, BlobStore, MediaSettings, Visibility};
use hypha_store_s3::{S3BlobStore, S3Config};

use crate::{
    error::ApiError,
    host::{HostClock, HostHttp},
};

pub type HostBlobs = S3BlobStore<HostHttp, HostClock>;

/// The file store and the URL rules; `None` in `AppState` without `media.s3.endpoint`,
/// and then the asset routes answer `503 media_not_configured`.
pub struct Media {
    pub blobs: HostBlobs,
    /// For signed URLs' expiry.
    clock: HostClock,
    pub urls: MediaUrls,
    /// URL lifetimes and the sweep's age (`[media]`).
    pub settings: MediaSettings,
}

/// How a thumbnail is made from a public image URL (`media.transforms`).
pub use hypha_config::Transforms;

/// Where clients fetch files from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaUrls {
    /// `media.public_url`, e.g. `https://cdn.mystore.com`: public files are served from
    /// here. Without it every file gets a signed URL (development, no CDN).
    pub public_base: Option<String>,
    pub transforms: Transforms,
}

/// The longest side of a thumbnail, in pixels.
pub const THUMBNAIL_WIDTH: u32 = 400;

impl MediaUrls {
    /// The public URL of `key` under the base, with Cloudflare's resizing options when
    /// given: `https://zone/cdn-cgi/image/<options>/<path>` (transformations live at the
    /// zone's root, whatever path the base has).
    fn public(&self, base: &str, key: &str, options: Option<&str>) -> String {
        let base = base.trim_end_matches('/');
        match options {
            None => format!("{base}/{key}"),
            Some(options) => {
                let (origin, path) = split_origin(base);
                format!("{origin}/cdn-cgi/image/{options}{path}/{key}")
            }
        }
    }
}

/// `https://zone.com/media` → (`https://zone.com`, `/media`).
fn split_origin(url: &str) -> (&str, &str) {
    let after_scheme = url.find("://").map_or(0, |i| i + 3);
    match url[after_scheme..].find('/') {
        Some(i) => url.split_at(after_scheme + i),
        None => (url, ""),
    }
}

impl Media {
    /// From `[media]`, with the S3 keys from `secret` (the environment natively, the
    /// Worker's secrets on Workers: secrets have no key in the file), sending through the
    /// host's HTTP, which `http` builds only when there's an S3 endpoint. `Ok(None)`
    /// without one.
    pub fn from_config(
        config: &hypha_config::MediaConfig,
        secret: impl Fn(&str) -> Option<String>,
        http: impl FnOnce() -> Result<HostHttp, String>,
        clock: HostClock,
    ) -> Result<Option<Media>, String> {
        let Some(s3) = &config.s3 else {
            return Ok(None);
        };
        let key = |name: &str| {
            secret(name).ok_or_else(|| format!("{name} is required with an S3 endpoint"))
        };
        let s3 = S3Config {
            endpoint: s3.endpoint.clone(),
            region: s3.region.clone(),
            bucket: s3.bucket.clone(),
            access_key_id: key("HYPHA_S3_ACCESS_KEY_ID")?,
            secret_access_key: key("HYPHA_S3_SECRET_ACCESS_KEY")?,
        };
        Ok(Some(Media {
            blobs: S3BlobStore::new(s3, http()?, clock.clone()),
            clock,
            urls: MediaUrls {
                public_base: config.public_url.clone(),
                transforms: config.transforms,
            },
            settings: config.settings,
        }))
    }

    /// The URL of the asset's file, or `None` until it is ready: public files under the
    /// public base, everything else signed for a while.
    pub async fn url(&self, asset: &Asset) -> Result<Option<String>, ApiError> {
        self.file_url(asset, None).await
    }

    /// A small version of an image (`THUMBNAIL_WIDTH` wide at most) when the URLs can
    /// resize; otherwise the original. `None` for other kinds and until ready.
    pub async fn thumbnail_url(&self, asset: &Asset) -> Result<Option<String>, ApiError> {
        if asset.kind() != AssetKind::Image {
            return Ok(None);
        }
        let options = match self.urls.transforms {
            Transforms::Cloudflare => Some(format!(
                "width={THUMBNAIL_WIDTH},fit=scale-down,format=auto"
            )),
            Transforms::None => None,
        };
        self.file_url(asset, options.as_deref()).await
    }

    async fn file_url(
        &self,
        asset: &Asset,
        options: Option<&str>,
    ) -> Result<Option<String>, ApiError> {
        if asset.status() != AssetStatus::Ready {
            return Ok(None);
        }
        match (&self.urls.public_base, asset.visibility()) {
            (Some(base), Visibility::Public) => {
                Ok(Some(self.urls.public(base, asset.key().as_str(), options)))
            }
            _ => {
                let now: Timestamp = self.clock.now();
                let url = self
                    .blobs
                    .presign_download(
                        asset.key(),
                        now,
                        MediaSettings::secs(self.settings.download_expires),
                    )
                    .await?;
                Ok(Some(url))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn urls(transforms: Transforms) -> MediaUrls {
        MediaUrls {
            public_base: Some("https://cdn.example.com/media/".to_string()),
            transforms,
        }
    }

    #[test]
    fn public_urls_join_the_base_and_the_key() {
        let urls = urls(Transforms::None);
        assert_eq!(
            urls.public("https://cdn.example.com/media/", "assets/1/1/a.png", None),
            "https://cdn.example.com/media/assets/1/1/a.png"
        );
    }

    #[test]
    fn cloudflare_options_go_at_the_zone_root() {
        let urls = urls(Transforms::Cloudflare);
        assert_eq!(
            urls.public(
                "https://cdn.example.com/media",
                "assets/1/1/a.png",
                Some("width=400")
            ),
            "https://cdn.example.com/cdn-cgi/image/width=400/media/assets/1/1/a.png"
        );
        assert_eq!(
            urls.public(
                "https://cdn.example.com",
                "assets/1/1/a.png",
                Some("width=400")
            ),
            "https://cdn.example.com/cdn-cgi/image/width=400/assets/1/1/a.png"
        );
    }
}
