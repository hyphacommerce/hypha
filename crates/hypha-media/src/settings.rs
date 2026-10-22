//! Media's settings: `[media]` in `hypha.config.toml`, defaults here.

use core::time::Duration;

const MINUTE: Duration = Duration::from_secs(60);

/// How long signed URLs work, and when unfinished uploads are swept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediaSettings {
    /// How long an upload URL works.
    pub upload_expires: Duration,
    /// How long a private file's download URL works.
    pub download_expires: Duration,
    /// How long an upload may stay unfinished (or a failed asset stay around) before
    /// `sweep_abandoned_uploads` deletes it. Longer than `upload_expires`, or an upload
    /// still in progress could be swept (the configuration checks it).
    pub abandoned_after: Duration,
}

impl Default for MediaSettings {
    fn default() -> Self {
        Self {
            upload_expires: 15 * MINUTE,
            download_expires: 15 * MINUTE,
            abandoned_after: 24 * 60 * MINUTE,
        }
    }
}

impl MediaSettings {
    /// A signed URL's lifetime in whole seconds, as `BlobStore` takes it (S3 allows up to
    /// 7 days; the configuration keeps it there).
    pub fn secs(duration: Duration) -> u32 {
        u32::try_from(duration.as_secs()).unwrap_or(u32::MAX)
    }
}
