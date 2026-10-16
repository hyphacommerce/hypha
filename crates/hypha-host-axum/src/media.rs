//! Media in the host (design note 0013): the file store (`HYPHA_S3_*`) and how asset URLs
//! are built (`HYPHA_MEDIA_URL`, `HYPHA_MEDIA_TRANSFORMS`).

use hypha_core::{Clock, Timestamp};
use hypha_media::{Asset, AssetKind, AssetStatus, BlobStore, MediaSettings, Visibility};
use hypha_store_s3::{ReqwestClient, S3BlobStore, S3Config};

use crate::{error::ApiError, state::SystemClock};

pub type HostBlobs = S3BlobStore<ReqwestClient, SystemClock>;

/// The file store and the URL rules; `None` in `AppState` when `HYPHA_S3_ENDPOINT`
/// isn't set, and then the asset routes answer `503 media_not_configured`.
pub struct Media {
    pub blobs: HostBlobs,
    pub urls: MediaUrls,
    /// URL lifetimes and the sweep's age (`[media]`, design note 0017).
    pub settings: MediaSettings,
}

/// How a thumbnail is made from a public image URL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transforms {
    /// No resizing: the thumbnail is the original.
    None,
    /// Cloudflare Image Transformations on the media domain's zone.
    Cloudflare,
}

/// Where clients fetch files from (design note 0013, section 4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaUrls {
    /// `HYPHA_MEDIA_URL`, e.g. `https://cdn.mystore.com`: public files are served from
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
    /// From `HYPHA_S3_*`, `HYPHA_MEDIA_URL` and `HYPHA_MEDIA_TRANSFORMS` (`cloudflare` or
    /// unset). `Ok(None)` without `HYPHA_S3_ENDPOINT`.
    pub fn from_env() -> Result<Option<Media>, String> {
        let Ok(endpoint) = std::env::var("HYPHA_S3_ENDPOINT") else {
            return Ok(None);
        };
        let var = |name: &str| {
            std::env::var(name).map_err(|_| format!("{name} is required with HYPHA_S3_ENDPOINT"))
        };
        let config = S3Config {
            endpoint,
            region: std::env::var("HYPHA_S3_REGION").unwrap_or_else(|_| "auto".to_string()),
            bucket: var("HYPHA_S3_BUCKET")?,
            access_key_id: var("HYPHA_S3_ACCESS_KEY_ID")?,
            secret_access_key: var("HYPHA_S3_SECRET_ACCESS_KEY")?,
        };
        let transforms = match std::env::var("HYPHA_MEDIA_TRANSFORMS").as_deref() {
            Err(_) | Ok("") | Ok("none") => Transforms::None,
            Ok("cloudflare") => Transforms::Cloudflare,
            Ok(other) => {
                return Err(format!(
                    "HYPHA_MEDIA_TRANSFORMS is `cloudflare` or `none`, not {other:?}"
                ));
            }
        };
        Ok(Some(Media {
            blobs: S3BlobStore::new(config, ReqwestClient::new()?, SystemClock),
            urls: MediaUrls {
                public_base: std::env::var("HYPHA_MEDIA_URL")
                    .ok()
                    .filter(|u| !u.is_empty()),
                transforms,
            },
            settings: MediaSettings::default(),
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
                let now: Timestamp = SystemClock.now();
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
