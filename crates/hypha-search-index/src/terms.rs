//! How text becomes search terms (design note 0019, section 4): one tokenizer for
//! documents and queries, shared by every engine, so they agree on what a word is.
//!
//! A term is a run of letters and digits, lowercased: "LS-RED" is `ls` and `red`,
//! "Čaša" is `čaša`. A query matches a document when every query term is a prefix of
//! some document term ("shir" finds "Shirt").

/// The longest term kept, in characters. Longer runs (encoded blobs, pasted hashes)
/// aren't words anyone searches for, and engines cap lexemes too (Postgres at 2047
/// bytes): dropping them here keeps every engine in agreement.
pub const MAX_TERM_CHARS: usize = 100;

/// The terms of `text`, in order, repeats kept.
pub fn terms(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty() && word.chars().count() <= MAX_TERM_CHARS)
        .map(str::to_lowercase)
        .collect()
}

/// `terms` joined with single spaces: what an engine with its own parser indexes, so
/// its parser sees nothing but plain words.
pub fn joined(text: &str) -> String {
    terms(text).join(" ")
}

/// Whether every query term is a prefix of one of `document`'s terms.
pub fn matches_all(query: &[String], document: &[String]) -> bool {
    query
        .iter()
        .all(|q| document.iter().any(|d| d.starts_with(q.as_str())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_on_anything_but_letters_and_digits() {
        assert_eq!(terms("LS-RED 100%_off"), ["ls", "red", "100", "off"]);
        assert_eq!(terms("  "), Vec::<String>::new());
    }

    #[test]
    fn words_longer_than_the_cap_are_dropped() {
        let long = "x".repeat(MAX_TERM_CHARS + 1);
        assert_eq!(terms(&format!("a {long} b")), ["a", "b"]);
        assert_eq!(terms(&"y".repeat(MAX_TERM_CHARS)).len(), 1);
    }

    #[test]
    fn lowercases_beyond_ascii() {
        assert_eq!(terms("ČAŠA"), ["čaša"]);
    }

    #[test]
    fn every_query_term_must_prefix_a_document_term() {
        let doc = terms("Linen Shirt LS-RED");
        assert!(matches_all(&terms("shir lin"), &doc));
        assert!(matches_all(&terms("ls-r"), &doc));
        assert!(!matches_all(&terms("shirt cotton"), &doc));
        assert!(
            !matches_all(&terms("hirt"), &doc),
            "a prefix, not any substring"
        );
    }
}
