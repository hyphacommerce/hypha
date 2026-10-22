//! What a product says about itself: its tags (its description is
//! `hypha_core::Description`).

use std::collections::HashSet;

use hypha_core::optional_text;

use crate::ProductError;

/// Most tags a product has (Shopify's limit).
pub const MAX_TAGS: usize = 250;

/// `tags` trimmed by the shared text rule, blanks dropped, duplicates (ignoring case)
/// dropped with the first spelling kept, in the order given.
pub fn tags(tags: Vec<String>) -> Result<Vec<String>, ProductError> {
    let mut seen = HashSet::new();
    let mut kept = Vec::new();
    for tag in tags {
        let Some(tag) = optional_text("tag", Some(tag))? else {
            continue;
        };
        if seen.insert(tag.to_lowercase()) {
            kept.push(tag);
        }
    }
    if kept.len() > MAX_TAGS {
        return Err(ProductError::TooManyTags { max: MAX_TAGS });
    }
    Ok(kept)
}

#[cfg(test)]
mod tests {
    use hypha_core::TextError;

    use super::*;

    #[test]
    fn tags_are_trimmed_and_deduplicated_ignoring_case() {
        assert_eq!(
            tags(vec![
                " Summer ".to_string(),
                "summer".to_string(),
                "".to_string(),
                "Linen".to_string(),
            ]),
            Ok(vec!["Summer".to_string(), "Linen".to_string()])
        );
    }

    #[test]
    fn tags_follow_the_text_rule() {
        assert_eq!(
            tags(vec!["x".repeat(256)]),
            Err(ProductError::Text(TextError::TooLong("tag")))
        );
    }

    #[test]
    fn at_most_250_distinct_tags() {
        let many = |n: usize| (0..n).map(|i| format!("t{i}")).collect::<Vec<_>>();
        assert_eq!(tags(many(MAX_TAGS)).map(|t| t.len()), Ok(MAX_TAGS));
        assert_eq!(
            tags(many(MAX_TAGS + 1)),
            Err(ProductError::TooManyTags { max: MAX_TAGS })
        );
        // Duplicates don't count.
        let mut repeated = many(MAX_TAGS);
        repeated.push("T0".to_string());
        assert_eq!(tags(repeated).map(|t| t.len()), Ok(MAX_TAGS));
    }
}
