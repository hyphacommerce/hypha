//! The file types Hypha accepts (design note 0013, section 3), and telling them apart by
//! their first bytes: what a client declares is checked against what it stored.

use std::{fmt, str::FromStr};

use crate::error::MediaError;

/// What an asset is, from its file type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AssetKind {
    Image,
    Video,
    File,
}

impl AssetKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            AssetKind::Image => "image",
            AssetKind::Video => "video",
            AssetKind::File => "file",
        }
    }

    /// The largest file of this kind, in bytes.
    pub fn max_bytes(self) -> u64 {
        const MB: u64 = 1024 * 1024;
        match self {
            AssetKind::Image => 20 * MB,
            AssetKind::Video => 1024 * MB,
            AssetKind::File => 100 * MB,
        }
    }
}

impl fmt::Display for AssetKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for AssetKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        [AssetKind::Image, AssetKind::Video, AssetKind::File]
            .into_iter()
            .find(|k| k.as_str() == s)
            .ok_or_else(|| s.to_string())
    }
}

/// An accepted file type. SVG is refused until it can be sanitised (0013, M12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MimeType {
    Jpeg,
    Png,
    Webp,
    Avif,
    Gif,
    Mp4,
    Webm,
    QuickTime,
    Pdf,
}

impl MimeType {
    pub const ALL: [MimeType; 9] = [
        MimeType::Jpeg,
        MimeType::Png,
        MimeType::Webp,
        MimeType::Avif,
        MimeType::Gif,
        MimeType::Mp4,
        MimeType::Webm,
        MimeType::QuickTime,
        MimeType::Pdf,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            MimeType::Jpeg => "image/jpeg",
            MimeType::Png => "image/png",
            MimeType::Webp => "image/webp",
            MimeType::Avif => "image/avif",
            MimeType::Gif => "image/gif",
            MimeType::Mp4 => "video/mp4",
            MimeType::Webm => "video/webm",
            MimeType::QuickTime => "video/quicktime",
            MimeType::Pdf => "application/pdf",
        }
    }

    pub fn kind(self) -> AssetKind {
        match self {
            MimeType::Jpeg | MimeType::Png | MimeType::Webp | MimeType::Avif | MimeType::Gif => {
                AssetKind::Image
            }
            MimeType::Mp4 | MimeType::Webm | MimeType::QuickTime => AssetKind::Video,
            MimeType::Pdf => AssetKind::File,
        }
    }

    /// The type a file's first bytes show, if it is one Hypha accepts.
    pub fn sniff(head: &[u8]) -> Option<MimeType> {
        let at =
            |offset: usize, magic: &[u8]| head.get(offset..offset + magic.len()) == Some(magic);
        if at(0, &[0xFF, 0xD8, 0xFF]) {
            Some(MimeType::Jpeg)
        } else if at(0, b"\x89PNG\r\n\x1a\n") {
            Some(MimeType::Png)
        } else if at(0, b"GIF87a") || at(0, b"GIF89a") {
            Some(MimeType::Gif)
        } else if at(0, b"RIFF") && at(8, b"WEBP") {
            Some(MimeType::Webp)
        } else if at(0, &[0x1A, 0x45, 0xDF, 0xA3]) {
            Some(MimeType::Webm)
        } else if at(0, b"%PDF-") {
            Some(MimeType::Pdf)
        } else if at(4, b"ftyp") {
            // ISO base media files (MP4, MOV, AVIF) name their format in the major brand.
            match head.get(8..12)? {
                b"avif" | b"avis" => Some(MimeType::Avif),
                b"qt  " => Some(MimeType::QuickTime),
                _ => Some(MimeType::Mp4),
            }
        } else {
            None
        }
    }
}

impl fmt::Display for MimeType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for MimeType {
    type Err = MediaError;

    /// Parameters (`; charset=…`) and case are ignored, as in a `Content-Type` header.
    fn from_str(s: &str) -> Result<Self, MediaError> {
        let essence = s
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        MimeType::ALL
            .into_iter()
            .find(|m| m.as_str() == essence)
            .ok_or_else(|| MediaError::UnsupportedMediaType(s.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepted_types_parse_with_parameters_and_any_case() {
        assert_eq!("IMAGE/PNG".parse(), Ok(MimeType::Png));
        assert_eq!("image/jpeg; q=1".parse(), Ok(MimeType::Jpeg));
    }

    #[test]
    fn svg_is_refused() {
        assert_eq!(
            "image/svg+xml".parse::<MimeType>(),
            Err(MediaError::UnsupportedMediaType(
                "image/svg+xml".to_string()
            ))
        );
    }

    #[test]
    fn kinds_follow_types() {
        assert_eq!(MimeType::Avif.kind(), AssetKind::Image);
        assert_eq!(MimeType::QuickTime.kind(), AssetKind::Video);
        assert_eq!(MimeType::Pdf.kind(), AssetKind::File);
    }

    #[test]
    fn files_are_told_apart_by_their_first_bytes() {
        let ftyp = |brand: &[u8]| [&[0, 0, 0, 0x20][..], b"ftyp", brand].concat();
        assert_eq!(
            MimeType::sniff(&[0xFF, 0xD8, 0xFF, 0xE0]),
            Some(MimeType::Jpeg)
        );
        assert_eq!(
            MimeType::sniff(b"\x89PNG\r\n\x1a\n...."),
            Some(MimeType::Png)
        );
        assert_eq!(MimeType::sniff(b"GIF89a"), Some(MimeType::Gif));
        assert_eq!(
            MimeType::sniff(b"RIFF\0\0\0\0WEBPVP8 "),
            Some(MimeType::Webp)
        );
        assert_eq!(MimeType::sniff(&ftyp(b"avif")), Some(MimeType::Avif));
        assert_eq!(MimeType::sniff(&ftyp(b"qt  ")), Some(MimeType::QuickTime));
        assert_eq!(MimeType::sniff(&ftyp(b"isom")), Some(MimeType::Mp4));
        assert_eq!(
            MimeType::sniff(&[0x1A, 0x45, 0xDF, 0xA3]),
            Some(MimeType::Webm)
        );
        assert_eq!(MimeType::sniff(b"%PDF-1.7"), Some(MimeType::Pdf));
        assert_eq!(MimeType::sniff(b"<svg xmlns="), None);
        assert_eq!(MimeType::sniff(b""), None);
    }
}
