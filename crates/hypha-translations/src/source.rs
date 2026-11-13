//! What a host tells the leaf about a resource: the fields it has now and their text in
//! the store's default language. The leaf can't see products or categories, so these are
//! plain values.

use sha2::{Digest, Sha256};

/// SHA-256 of a source text, in lowercase hex: what a translated field remembers of the
/// text it translated.
pub fn source_digest(text: &str) -> String {
    let hash = Sha256::digest(text.as_bytes());
    let mut hex = String::with_capacity(hash.len() * 2);
    for byte in hash {
        hex.push(char::from_digit(u32::from(byte >> 4), 16).unwrap_or('0'));
        hex.push(char::from_digit(u32::from(byte & 0x0f), 16).unwrap_or('0'));
    }
    hex
}

/// One translatable field of a resource as it is now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceField {
    /// Its key (`title`, `options.<id>.name`).
    pub key: String,
    /// The text in the store's default language; empty when the resource has none (a
    /// product without a description).
    pub text: String,
    /// `source_digest(text)`.
    pub digest: String,
}

impl SourceField {
    pub fn new(key: impl Into<String>, text: Option<&str>) -> Self {
        let text = text.unwrap_or_default().to_string();
        Self {
            key: key.into(),
            digest: source_digest(&text),
            text,
        }
    }
}

/// A resource's translatable fields now, in the order an editor shows them: every field
/// of its own and one per field of each child it has. A key not listed can't be
/// translated (its child is gone).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Source {
    pub fields: Vec<SourceField>,
}

impl Source {
    pub fn new(fields: Vec<SourceField>) -> Self {
        Self { fields }
    }

    pub fn field(&self, key: &str) -> Option<&SourceField> {
        self.fields.iter().find(|f| f.key == key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_digest_is_sha_256_in_lowercase_hex() {
        assert_eq!(
            source_digest("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            source_digest(""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn a_field_without_text_digests_the_empty_text() {
        let field = SourceField::new("description", None);
        assert_eq!(field.text, "");
        assert_eq!(field.digest, source_digest(""));
    }
}
