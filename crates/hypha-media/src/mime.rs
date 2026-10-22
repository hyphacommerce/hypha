//! The file types Hypha accepts, and telling them apart by their first bytes: what a client
//! declares is checked against what it stored.

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

/// An accepted file type. SVG is refused until it can be sanitised.
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
            iso_media(head)
        } else {
            None
        }
    }
}

/// An ISO base media file (MOV, AVIF, but also HEIC and M4A) by its `ftyp` box: the major
/// brand and the compatible brands after it say what it is.
fn iso_media(head: &[u8]) -> Option<MimeType> {
    let size = u32::from_be_bytes(head.get(0..4)?.try_into().ok()?) as usize;
    let ftyp = head.get(8..size.min(head.len()))?;
    let major = ftyp.get(0..4)?;
    // After the major brand come the minor version (4 bytes) and the compatible brands.
    let (compatible, _) = ftyp.get(8..).unwrap_or_default().as_chunks::<4>();
    let brands: Vec<&[u8]> = std::iter::once(major)
        .chain(compatible.iter().map(|brand| brand.as_slice()))
        .collect();
    let has = |brand: &[u8]| brands.contains(&brand);
    if has(b"avif") || has(b"avis") {
        return Some(MimeType::Avif);
    }
    // Audio and still-image formats that also list MP4 brands: not videos.
    const NOT_VIDEO: [&[u8]; 10] = [
        b"M4A ", b"M4B ", b"M4P ", b"heic", b"heix", b"hevc", b"heim", b"heis", b"mif1", b"msf1",
    ];
    if NOT_VIDEO.contains(&major) {
        return None;
    }
    if major == b"qt  " {
        return Some(MimeType::QuickTime);
    }
    const VIDEO: [&[u8]; 11] = [
        b"isom", b"iso2", b"iso4", b"iso5", b"iso6", b"mp41", b"mp42", b"avc1", b"M4V ", b"dash",
        b"mmp4",
    ];
    brands
        .iter()
        .any(|brand| VIDEO.contains(brand))
        .then_some(MimeType::Mp4)
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

    /// An `ftyp` box with a major brand and compatible brands.
    fn ftyp_box(major: &[u8], compatible: &[&[u8]]) -> Vec<u8> {
        let size = (16 + 4 * compatible.len()) as u32;
        let mut boxed = size.to_be_bytes().to_vec();
        boxed.extend_from_slice(b"ftyp");
        boxed.extend_from_slice(major);
        boxed.extend_from_slice(&[0, 0, 0, 0]);
        for brand in compatible {
            boxed.extend_from_slice(brand);
        }
        boxed
    }

    #[test]
    fn iso_files_are_told_apart_by_all_their_brands() {
        // AVIF exported with a generic major brand, AVIF named in the compatible ones.
        assert_eq!(
            MimeType::sniff(&ftyp_box(b"mif1", &[b"mif1", b"avif", b"miaf"])),
            Some(MimeType::Avif)
        );
        assert_eq!(
            MimeType::sniff(&ftyp_box(b"heic", &[b"mif1", b"heic"])),
            None,
            "HEIC isn't accepted"
        );
        assert_eq!(
            MimeType::sniff(&ftyp_box(b"M4A ", &[b"M4A ", b"mp42", b"isom"])),
            None,
            "audio isn't a video"
        );
        assert_eq!(
            MimeType::sniff(&ftyp_box(b"XAVC", &[b"mp42", b"iso2"])),
            Some(MimeType::Mp4),
            "an unknown major brand that is compatible with MP4"
        );
        assert_eq!(MimeType::sniff(&ftyp_box(b"crx ", &[])), None);
    }

    #[test]
    fn files_are_told_apart_by_their_first_bytes() {
        let ftyp = |brand: &[u8]| ftyp_box(brand, &[]);
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
