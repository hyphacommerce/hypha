use std::{fmt, str::FromStr};

use hypha_core::{
    AssetId, PendingEvent, Recorded, Recorder, Subject, Timestamp, Version, Versioned,
    optional_text, required_text,
};

use crate::{
    AssetKind, BlobKey, MediaError, MimeType, UploadMismatch,
    events::{ASSET, AssetDeleted},
};

/// Where an asset is in its upload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AssetStatus {
    /// Created; the file isn't uploaded or checked yet.
    Pending,
    /// The stored file is what was declared. Only ready assets are listed and attached.
    Ready,
    /// The stored file wasn't what was declared; it was deleted.
    Failed,
}

impl AssetStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            AssetStatus::Pending => "pending",
            AssetStatus::Ready => "ready",
            AssetStatus::Failed => "failed",
        }
    }
}

impl fmt::Display for AssetStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for AssetStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        [
            AssetStatus::Pending,
            AssetStatus::Ready,
            AssetStatus::Failed,
        ]
        .into_iter()
        .find(|v| v.as_str() == s)
        .ok_or_else(|| s.to_string())
    }
}

/// Public files get a CDN URL; private ones only short-lived signed URLs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Visibility {
    Public,
    Private,
}

impl Visibility {
    pub fn as_str(&self) -> &'static str {
        match self {
            Visibility::Public => "public",
            Visibility::Private => "private",
        }
    }

    /// What storage sends back with every download. A public file's URL never changes
    /// content (a new file is a new version and key), so caches may keep it for a year.
    pub fn cache_control(self) -> &'static str {
        match self {
            Visibility::Public => "public, max-age=31536000, immutable",
            Visibility::Private => "private, no-store",
        }
    }
}

impl fmt::Display for Visibility {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Visibility {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        [Visibility::Public, Visibility::Private]
            .into_iter()
            .find(|v| v.as_str() == s)
            .ok_or_else(|| s.to_string())
    }
}

/// A file's SHA-256, declared by the uploader. Storage checks the upload against it, and
/// the library uses it to find duplicates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Checksum([u8; 32]);

impl Checksum {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// 64 hex characters, either case.
    pub fn from_hex(hex: &str) -> Result<Self, MediaError> {
        let invalid = || MediaError::InvalidChecksum(hex.to_string());
        // Checked first: `from_str_radix` would also take a sign (`+a`).
        if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(invalid());
        }
        let mut bytes = [0u8; 32];
        for (i, byte) in bytes.iter_mut().enumerate() {
            let pair = hex.get(i * 2..i * 2 + 2).ok_or_else(invalid)?;
            *byte = u8::from_str_radix(pair, 16).map_err(|_| invalid())?;
        }
        Ok(Self(bytes))
    }
}

impl fmt::Display for Checksum {
    /// Lowercase hex.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.iter().try_for_each(|b| write!(f, "{b:02x}"))
    }
}

/// What the file itself says, read when its upload is completed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FileFacts {
    pub width: Option<u32>,
    pub height: Option<u32>,
}

/// How many bytes from the start of a file `verify_upload` needs to see: enough for the
/// type and, for images, the dimensions (a JPEG's can sit behind large metadata).
pub const HEAD_BYTES: u64 = 256 * 1024;

/// A new asset waiting for its file.
pub struct NewUpload {
    pub id: AssetId,
    pub filename: String,
    pub mime_type: MimeType,
    pub byte_size: u64,
    pub checksum: Option<Checksum>,
    pub visibility: Visibility,
    pub now: Timestamp,
}

/// One file and what is known about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    id: AssetId,
    version: Version,
    status: AssetStatus,
    filename: String,
    key: BlobKey,
    file_version: u32,
    mime_type: MimeType,
    byte_size: u64,
    checksum: Option<Checksum>,
    width: Option<u32>,
    height: Option<u32>,
    alt: Option<String>,
    title: Option<String>,
    visibility: Visibility,
    created_at: Timestamp,
    updated_at: Timestamp,
    /// What happened since it was loaded, for its events.
    events: Recorder,
}

impl Recorded for Asset {
    fn take_events(&mut self) -> Vec<PendingEvent> {
        self.events.take()
    }
}

impl Versioned for Asset {
    fn version(&self) -> Version {
        self.version
    }

    fn set_version(&mut self, version: Version) {
        self.version = version;
    }
}

impl Asset {
    /// Records that the asset is being deleted (`asset.deleted`), for `AssetRepo::delete`
    /// to write with the row.
    pub fn delete(&mut self) {
        let event = AssetDeleted { asset: self.id };
        self.events
            .record(Subject::new(ASSET, self.id.as_uuid()), &event);
    }

    pub fn new_upload(new: NewUpload) -> Result<Self, MediaError> {
        let kind = new.mime_type.kind();
        if new.byte_size == 0 {
            return Err(MediaError::EmptyFile);
        }
        if new.byte_size > kind.max_bytes() {
            return Err(MediaError::FileTooLarge {
                kind,
                max: kind.max_bytes(),
            });
        }
        let filename = required_text("filename", &new.filename)?;
        let file_version = 1;
        Ok(Self {
            id: new.id,
            version: Version::NEW,
            status: AssetStatus::Pending,
            key: blob_key(new.visibility, new.id, file_version, &filename),
            filename,
            file_version,
            mime_type: new.mime_type,
            byte_size: new.byte_size,
            checksum: new.checksum,
            width: None,
            height: None,
            alt: None,
            title: None,
            visibility: new.visibility,
            created_at: new.now,
            updated_at: new.now,
            events: Recorder::default(),
        })
    }

    /// Checks the stored file against what was declared: its size, and its type from its
    /// first bytes (`head`, at least `HEAD_BYTES` of them when the file is that long).
    /// Images also give their dimensions.
    pub fn verify_upload(
        &self,
        stored_size: u64,
        head: &[u8],
    ) -> Result<FileFacts, UploadMismatch> {
        if stored_size != self.byte_size {
            return Err(UploadMismatch::Size {
                declared: self.byte_size,
                stored: stored_size,
            });
        }
        if MimeType::sniff(head) != Some(self.mime_type) {
            return Err(UploadMismatch::Type {
                declared: self.mime_type,
            });
        }
        if self.kind() != AssetKind::Image {
            return Ok(FileFacts::default());
        }
        // Dimensions the header doesn't reach are left unknown rather than failing.
        Ok(imagesize::blob_size(head)
            .ok()
            .and_then(|size| {
                Some(FileFacts {
                    width: Some(u32::try_from(size.width).ok()?),
                    height: Some(u32::try_from(size.height).ok()?),
                })
            })
            .unwrap_or_default())
    }

    /// The file checked out: the asset can be listed and attached.
    pub fn mark_ready(&mut self, facts: FileFacts, now: Timestamp) -> Result<(), MediaError> {
        self.ensure_pending()?;
        self.status = AssetStatus::Ready;
        self.width = facts.width;
        self.height = facts.height;
        self.updated_at = now;
        Ok(())
    }

    /// The file didn't check out (and was deleted).
    pub fn mark_failed(&mut self, now: Timestamp) -> Result<(), MediaError> {
        self.ensure_pending()?;
        self.status = AssetStatus::Failed;
        self.updated_at = now;
        Ok(())
    }

    fn ensure_pending(&self) -> Result<(), MediaError> {
        match self.status {
            AssetStatus::Pending => Ok(()),
            status => Err(MediaError::NotPending { status }),
        }
    }

    /// Trimmed; blank clears it.
    pub fn set_alt(&mut self, alt: Option<String>, now: Timestamp) -> Result<(), MediaError> {
        self.alt = optional_text("alt", alt)?;
        self.updated_at = now;
        Ok(())
    }

    /// Trimmed; blank clears it.
    pub fn set_title(&mut self, title: Option<String>, now: Timestamp) -> Result<(), MediaError> {
        self.title = optional_text("title", title)?;
        self.updated_at = now;
        Ok(())
    }

    pub fn id(&self) -> AssetId {
        self.id
    }

    pub fn status(&self) -> AssetStatus {
        self.status
    }

    pub fn kind(&self) -> AssetKind {
        self.mime_type.kind()
    }

    /// As uploaded (trimmed), for people; the key uses a sanitised copy.
    pub fn filename(&self) -> &str {
        &self.filename
    }

    /// Where the file is served from once ready: `assets/…` or `private/…`.
    pub fn key(&self) -> &BlobKey {
        &self.key
    }

    /// Where the client uploads the file: `uploads/…`, same ID, version and name. Hypha
    /// copies it to `key` once checked, so the upload URL (valid for a while) can never
    /// replace a file that is being served.
    pub fn upload_key(&self) -> BlobKey {
        let rest = self
            .key
            .as_str()
            .split_once('/')
            .map_or(self.key.as_str(), |(_, rest)| rest);
        BlobKey::restore(format!("uploads/{rest}"))
    }

    /// Starts at 1; a replaced file gets the next one, and a new key.
    pub fn file_version(&self) -> u32 {
        self.file_version
    }

    pub fn mime_type(&self) -> MimeType {
        self.mime_type
    }

    pub fn byte_size(&self) -> u64 {
        self.byte_size
    }

    pub fn checksum(&self) -> Option<Checksum> {
        self.checksum
    }

    pub fn width(&self) -> Option<u32> {
        self.width
    }

    pub fn height(&self) -> Option<u32> {
        self.height
    }

    pub fn alt(&self) -> Option<&str> {
        self.alt.as_deref()
    }

    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    pub fn visibility(&self) -> Visibility {
        self.visibility
    }

    pub fn created_at(&self) -> Timestamp {
        self.created_at
    }

    pub fn updated_at(&self) -> Timestamp {
        self.updated_at
    }
}

/// `assets/{id}/{version}/{name}` (public) or `private/{id}/{version}/{name}`: unique per
/// file version, so a URL's content never changes, with a readable name.
fn blob_key(visibility: Visibility, id: AssetId, file_version: u32, filename: &str) -> BlobKey {
    let prefix = match visibility {
        Visibility::Public => "assets",
        Visibility::Private => "private",
    };
    let id = id.to_string();
    let id = id.strip_prefix("asset_").unwrap_or(&id);
    BlobKey::restore(format!(
        "{prefix}/{id}/{file_version}/{}",
        key_name(filename)
    ))
}

/// `filename` made safe for keys and URLs: lowercase `[a-z0-9._-]`, other runs of
/// characters as one `-` (none next to a `.`), at most 100 characters with the
/// extension kept; `file` when nothing of the name is left.
fn key_name(filename: &str) -> String {
    // The extension is kept apart, so shortening a long name never cuts it off.
    let (stem, extension) = match filename.rsplit_once('.') {
        Some((stem, ext)) if !ext.is_empty() && ext.len() <= 10 => (stem, safe_part(ext)),
        _ => (filename, String::new()),
    };
    let room = if extension.is_empty() {
        100
    } else {
        100 - extension.len() - 1
    };
    let stem: String = safe_part(stem).chars().take(room).collect();
    let stem = stem.trim_end_matches(['-', '.']);
    let stem = if stem.is_empty() { "file" } else { stem };
    if extension.is_empty() {
        stem.to_string()
    } else {
        format!("{stem}.{extension}")
    }
}

/// Lowercase `[a-z0-9._-]`, other runs of characters as one `-`, no `-` next to a `.`,
/// nothing to trim at either end.
fn safe_part(part: &str) -> String {
    let mut out = String::new();
    for c in part.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() || c == '.' || c == '_' {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.replace("-.", ".").replace(".-", ".");
    out.trim_matches(['-', '.']).to_string()
}

/// Plain-data form of an `Asset` for storage adapters; `restore` re-checks the rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetSnapshot {
    pub id: AssetId,
    pub version: Version,
    pub status: AssetStatus,
    pub filename: String,
    pub key: BlobKey,
    pub file_version: u32,
    pub mime_type: MimeType,
    pub byte_size: u64,
    pub checksum: Option<Checksum>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub alt: Option<String>,
    pub title: Option<String>,
    pub visibility: Visibility,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl Asset {
    pub fn snapshot(&self) -> AssetSnapshot {
        AssetSnapshot {
            id: self.id,
            version: self.version,
            status: self.status,
            filename: self.filename.clone(),
            key: self.key.clone(),
            file_version: self.file_version,
            mime_type: self.mime_type,
            byte_size: self.byte_size,
            checksum: self.checksum,
            width: self.width,
            height: self.height,
            alt: self.alt.clone(),
            title: self.title.clone(),
            visibility: self.visibility,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }

    pub fn restore(s: AssetSnapshot) -> Result<Asset, MediaError> {
        let kind = s.mime_type.kind();
        if s.byte_size == 0 {
            return Err(MediaError::EmptyFile);
        }
        if s.byte_size > kind.max_bytes() {
            return Err(MediaError::FileTooLarge {
                kind,
                max: kind.max_bytes(),
            });
        }
        Ok(Asset {
            id: s.id,
            version: s.version,
            status: s.status,
            filename: required_text("filename", &s.filename)?,
            key: s.key,
            file_version: s.file_version,
            mime_type: s.mime_type,
            byte_size: s.byte_size,
            checksum: s.checksum,
            width: s.width,
            height: s.height,
            alt: optional_text("alt", s.alt)?,
            title: optional_text("title", s.title)?,
            visibility: s.visibility,
            created_at: s.created_at,
            updated_at: s.updated_at,
            events: Recorder::default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use hypha_core::TextError;
    use uuid::Uuid;

    use super::*;

    const NOW: Timestamp = Timestamp::from_unix_micros(1_767_225_600_000_000);

    fn id() -> AssetId {
        AssetId::from_uuid(Uuid::from_u128(1))
    }

    fn upload(filename: &str, mime_type: MimeType, byte_size: u64) -> Result<Asset, MediaError> {
        upload_as(Visibility::Public, filename, mime_type, byte_size)
    }

    fn upload_as(
        visibility: Visibility,
        filename: &str,
        mime_type: MimeType,
        byte_size: u64,
    ) -> Result<Asset, MediaError> {
        Asset::new_upload(NewUpload {
            id: id(),
            filename: filename.to_string(),
            mime_type,
            byte_size,
            checksum: None,
            visibility,
            now: NOW,
        })
    }

    /// The first bytes of a 3 × 2 PNG: signature and IHDR.
    fn png_head() -> Vec<u8> {
        let mut head = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        head.extend_from_slice(&3u32.to_be_bytes());
        head.extend_from_slice(&2u32.to_be_bytes());
        head.extend_from_slice(&[8, 6, 0, 0, 0]);
        head
    }

    #[test]
    fn a_new_upload_is_pending_under_a_readable_versioned_key() {
        let asset = upload("Summer Shirt (front).JPG", MimeType::Jpeg, 10).unwrap();
        assert_eq!(asset.status(), AssetStatus::Pending);
        assert_eq!(asset.filename(), "Summer Shirt (front).JPG");
        assert_eq!(
            asset.key().as_str(),
            "assets/00000000000000000000000000000001/1/summer-shirt-front.jpg"
        );
        assert_eq!(
            asset.upload_key().as_str(),
            "uploads/00000000000000000000000000000001/1/summer-shirt-front.jpg"
        );
    }

    #[test]
    fn private_files_are_kept_apart() {
        let asset = upload_as(Visibility::Private, "invoice.pdf", MimeType::Pdf, 10).unwrap();
        assert_eq!(
            asset.key().as_str(),
            "private/00000000000000000000000000000001/1/invoice.pdf"
        );
    }

    #[test]
    fn key_names_keep_only_safe_characters() {
        assert_eq!(key_name("Été à la plage.webp"), "t-la-plage.webp");
        assert_eq!(key_name("--.."), "file");
        assert_eq!(key_name(&"a".repeat(300)).len(), 100);
        assert_eq!(key_name("-.png"), "file.png");
    }

    #[test]
    fn long_names_keep_their_extension() {
        let long = format!("{}-v2.jpg", "long-product-name-".repeat(10));
        let name = key_name(&long);
        assert_eq!(name.len(), 100);
        assert!(name.ends_with(".jpg"), "{name}");
        assert!(!name.contains("-."), "{name}");
    }

    #[test]
    fn sizes_must_fit_the_kind() {
        assert_eq!(
            upload("a.png", MimeType::Png, 0),
            Err(MediaError::EmptyFile)
        );
        assert!(upload("a.png", MimeType::Png, AssetKind::Image.max_bytes()).is_ok());
        assert_eq!(
            upload("a.png", MimeType::Png, AssetKind::Image.max_bytes() + 1),
            Err(MediaError::FileTooLarge {
                kind: AssetKind::Image,
                max: AssetKind::Image.max_bytes()
            })
        );
    }

    #[test]
    fn a_filename_is_required() {
        assert_eq!(
            upload("  ", MimeType::Png, 1),
            Err(MediaError::Text(TextError::Missing("filename")))
        );
    }

    #[test]
    fn a_matching_upload_gives_the_image_dimensions() {
        let head = png_head();
        let asset = upload("a.png", MimeType::Png, 100).unwrap();
        assert_eq!(
            asset.verify_upload(100, &head),
            Ok(FileFacts {
                width: Some(3),
                height: Some(2)
            })
        );
    }

    #[test]
    fn a_different_size_or_type_is_a_mismatch() {
        let asset = upload("a.png", MimeType::Png, 100).unwrap();
        assert_eq!(
            asset.verify_upload(99, &png_head()),
            Err(UploadMismatch::Size {
                declared: 100,
                stored: 99
            })
        );
        assert_eq!(
            asset.verify_upload(100, b"%PDF-1.7"),
            Err(UploadMismatch::Type {
                declared: MimeType::Png
            })
        );
    }

    #[test]
    fn only_a_pending_asset_becomes_ready_or_failed() {
        let mut asset = upload("a.png", MimeType::Png, 1).unwrap();
        asset.mark_ready(FileFacts::default(), NOW).unwrap();
        assert_eq!(
            asset.mark_failed(NOW),
            Err(MediaError::NotPending {
                status: AssetStatus::Ready
            })
        );
    }

    #[test]
    fn checksums_are_64_hex_characters() {
        let hex = "ab".repeat(32);
        assert_eq!(Checksum::from_hex(&hex).unwrap().to_string(), hex);
        assert_eq!(
            Checksum::from_hex(&hex.to_uppercase()).map(|c| c.to_string()),
            Ok(hex.clone())
        );
        assert_eq!(
            Checksum::from_hex("abc"),
            Err(MediaError::InvalidChecksum("abc".to_string()))
        );
        let signed = "+a".repeat(32);
        assert_eq!(
            Checksum::from_hex(&signed),
            Err(MediaError::InvalidChecksum(signed.clone()))
        );
        let not_hex = "zz".repeat(32);
        assert_eq!(
            Checksum::from_hex(&not_hex),
            Err(MediaError::InvalidChecksum(not_hex.clone()))
        );
    }

    #[test]
    fn alt_text_is_trimmed_and_blank_clears_it() {
        let mut asset = upload("a.png", MimeType::Png, 1).unwrap();
        asset
            .set_alt(Some("  A blue shirt ".to_string()), NOW)
            .unwrap();
        assert_eq!(asset.alt(), Some("A blue shirt"));
        asset.set_alt(Some(" ".to_string()), NOW).unwrap();
        assert_eq!(asset.alt(), None);
    }

    #[test]
    fn a_snapshot_restores_the_same_asset() {
        let asset = upload("a.png", MimeType::Png, 1).unwrap();
        assert_eq!(Asset::restore(asset.snapshot()), Ok(asset));
    }
}
