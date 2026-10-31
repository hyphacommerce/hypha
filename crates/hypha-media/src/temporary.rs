//! Temporary uploads: a file a client uploads straight to storage for Hypha's server side
//! to read back once (a CSV of prices to import), kept for `TemporaryUpload::LIFETIME` and
//! swept after. Unlike assets they're never served, listed or attached.

use core::time::Duration;
use std::{fmt, str::FromStr};

use hypha_core::{Scope, Timestamp, UploadId, required_text};

use crate::{BlobKey, Checksum, MediaError};

/// What a temporary upload is for: which scope may make and read it, what type of file it
/// is and how large it may be.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UploadPurpose {
    /// Fixed prices to import into a price list, as CSV.
    PriceImport,
}

impl UploadPurpose {
    pub const ALL: [UploadPurpose; 1] = [UploadPurpose::PriceImport];

    pub fn as_str(&self) -> &'static str {
        match self {
            UploadPurpose::PriceImport => "price_import",
        }
    }

    /// The scope that makes, reads and deletes uploads of this purpose.
    pub fn scope(&self) -> Scope {
        match self {
            UploadPurpose::PriceImport => Scope::PricingWrite,
        }
    }

    /// The file's type, signed into the upload: storage refuses another.
    pub fn content_type(&self) -> &'static str {
        match self {
            UploadPurpose::PriceImport => "text/csv",
        }
    }

    /// The largest file, in bytes.
    pub fn max_bytes(&self) -> u64 {
        const MB: u64 = 1024 * 1024;
        match self {
            UploadPurpose::PriceImport => 20 * MB,
        }
    }

    fn extension(self) -> &'static str {
        match self {
            UploadPurpose::PriceImport => "csv",
        }
    }
}

impl fmt::Display for UploadPurpose {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for UploadPurpose {
    type Err = MediaError;

    fn from_str(s: &str) -> Result<Self, MediaError> {
        Self::ALL
            .into_iter()
            .find(|p| p.as_str() == s)
            .ok_or_else(|| MediaError::UnknownPurpose(s.to_string()))
    }
}

/// A file uploaded for a while.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemporaryUpload {
    id: UploadId,
    purpose: UploadPurpose,
    filename: String,
    byte_size: u64,
    checksum: Option<Checksum>,
    created_at: Timestamp,
    expires_at: Timestamp,
}

/// A new temporary upload as a caller describes it.
pub struct NewTemporaryUpload {
    pub id: UploadId,
    pub purpose: UploadPurpose,
    pub filename: String,
    pub byte_size: u64,
    pub checksum: Option<Checksum>,
    pub now: Timestamp,
}

/// A stored temporary upload, for adapters reading it back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemporaryUploadSnapshot {
    pub id: UploadId,
    pub purpose: UploadPurpose,
    pub filename: String,
    pub byte_size: u64,
    pub checksum: Option<Checksum>,
    pub created_at: Timestamp,
    pub expires_at: Timestamp,
}

impl TemporaryUpload {
    /// How long one is kept: long enough to upload it and import it, not to keep files.
    pub const LIFETIME: Duration = Duration::from_secs(24 * 60 * 60);

    /// Not empty, at most the purpose's `max_bytes`; the filename follows the shared text
    /// rule (it's kept for people, never part of the key).
    pub fn new(new: NewTemporaryUpload) -> Result<Self, MediaError> {
        if new.byte_size == 0 {
            return Err(MediaError::EmptyFile);
        }
        let max = new.purpose.max_bytes();
        if new.byte_size > max {
            return Err(MediaError::UploadTooLarge {
                purpose: new.purpose,
                max,
            });
        }
        Ok(Self {
            id: new.id,
            purpose: new.purpose,
            filename: required_text("filename", &new.filename)?,
            byte_size: new.byte_size,
            checksum: new.checksum,
            created_at: new.now,
            expires_at: new.now.plus(Self::LIFETIME),
        })
    }

    pub fn restore(s: TemporaryUploadSnapshot) -> Self {
        Self {
            id: s.id,
            purpose: s.purpose,
            filename: s.filename,
            byte_size: s.byte_size,
            checksum: s.checksum,
            created_at: s.created_at,
            expires_at: s.expires_at,
        }
    }

    pub fn snapshot(&self) -> TemporaryUploadSnapshot {
        TemporaryUploadSnapshot {
            id: self.id,
            purpose: self.purpose,
            filename: self.filename.clone(),
            byte_size: self.byte_size,
            checksum: self.checksum,
            created_at: self.created_at,
            expires_at: self.expires_at,
        }
    }

    /// Where the client uploads the file: `temporary/<id>.<extension>`, apart from the
    /// assets' keys, and made of `[a-z0-9./]` only.
    pub fn key(&self) -> BlobKey {
        let id = self.id.to_string();
        let id = id.strip_prefix("upl_").unwrap_or(&id);
        BlobKey::restore(format!("temporary/{id}.{}", self.purpose.extension()))
    }

    /// Whether it's past its lifetime at `now`: gone, as far as callers can tell.
    pub fn is_expired(&self, now: Timestamp) -> bool {
        now >= self.expires_at
    }

    pub fn id(&self) -> UploadId {
        self.id
    }

    pub fn purpose(&self) -> UploadPurpose {
        self.purpose
    }

    pub fn filename(&self) -> &str {
        &self.filename
    }

    pub fn byte_size(&self) -> u64 {
        self.byte_size
    }

    pub fn checksum(&self) -> Option<Checksum> {
        self.checksum
    }

    pub fn created_at(&self) -> Timestamp {
        self.created_at
    }

    pub fn expires_at(&self) -> Timestamp {
        self.expires_at
    }
}

#[cfg(test)]
mod tests {
    use hypha_core::TextError;
    use uuid::Uuid;

    use super::*;

    fn new(byte_size: u64) -> NewTemporaryUpload {
        NewTemporaryUpload {
            id: UploadId::from_uuid(Uuid::from_u128(1)),
            purpose: UploadPurpose::PriceImport,
            filename: "prices.csv".to_string(),
            byte_size,
            checksum: None,
            now: Timestamp::from_unix_micros(0),
        }
    }

    #[test]
    fn a_purpose_reads_back_from_its_name() {
        assert_eq!("price_import".parse(), Ok(UploadPurpose::PriceImport));
        assert_eq!(
            "avatar".parse::<UploadPurpose>(),
            Err(MediaError::UnknownPurpose("avatar".to_string()))
        );
    }

    #[test]
    fn a_price_import_is_at_most_twenty_megabytes() {
        assert!(TemporaryUpload::new(new(20 * 1024 * 1024)).is_ok());
        assert_eq!(
            TemporaryUpload::new(new(20 * 1024 * 1024 + 1)),
            Err(MediaError::UploadTooLarge {
                purpose: UploadPurpose::PriceImport,
                max: 20 * 1024 * 1024
            })
        );
        assert_eq!(TemporaryUpload::new(new(0)), Err(MediaError::EmptyFile));
    }

    #[test]
    fn the_filename_is_kept_for_people_but_never_in_the_key() {
        let upload = TemporaryUpload::new(NewTemporaryUpload {
            filename: " ../Prices (Q4).csv ".to_string(),
            ..new(10)
        })
        .unwrap();
        assert_eq!(upload.filename(), "../Prices (Q4).csv");
        assert_eq!(
            upload.key().as_str(),
            "temporary/00000000000000000000000000000001.csv"
        );
        assert_eq!(
            TemporaryUpload::new(NewTemporaryUpload {
                filename: " ".to_string(),
                ..new(10)
            }),
            Err(MediaError::Text(TextError::Missing("filename")))
        );
    }

    #[test]
    fn it_expires_after_a_day() {
        let upload = TemporaryUpload::new(new(10)).unwrap();
        let day = Timestamp::from_unix_micros(0).plus(TemporaryUpload::LIFETIME);
        assert_eq!(upload.expires_at(), day);
        assert!(!upload.is_expired(day.minus(Duration::from_micros(1))));
        assert!(upload.is_expired(day));
    }
}
