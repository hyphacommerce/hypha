//! What a product says about itself (design note 0019, section 2): its description,
//! vendor, product type and tags, the material search and facets are made of.

use std::collections::HashSet;

use hypha_core::optional_text;

use crate::ProductError;

/// A product description: HTML, sanitised when it's made, so what's stored is always
/// safe to render (design note 0019, Q1). An allow-list keeps paragraphs, headings,
/// lists, emphasis, links, tables and images, and drops scripts, styles, event
/// attributes and unsafe URLs (`ammonia`'s defaults).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Description(String);

impl Description {
    /// Longest description accepted, in bytes of the HTML sent.
    pub const MAX_LEN: usize = 64 * 1024;

    /// Sanitises `html`. `None` when nothing is left (blank, or only markup that was
    /// removed).
    pub fn new(html: &str) -> Result<Option<Self>, ProductError> {
        if html.len() > Self::MAX_LEN {
            return Err(ProductError::DescriptionTooLong { max: Self::MAX_LEN });
        }
        let clean = ammonia::clean(html);
        let clean = clean.trim();
        if text_of(clean).is_empty() && !clean.contains("<img") {
            return Ok(None);
        }
        Ok(Some(Self(clean.to_string())))
    }

    pub fn as_html(&self) -> &str {
        &self.0
    }

    /// The words, with the markup removed and whitespace collapsed: what search matches.
    pub fn text(&self) -> String {
        text_of(&self.0)
    }
}

/// The text of sanitised HTML. Its serialiser writes every attribute in double quotes
/// with `"` escaped, so a tag ends at the first `>` outside quotes. Each tag becomes a
/// space, so `<p>a</p><p>b</p>` is two words.
fn text_of(html: &str) -> String {
    let mut text = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut in_quotes = false;
    for c in html.chars() {
        match (in_tag, c) {
            (false, '<') => {
                in_tag = true;
                text.push(' ');
            }
            (false, c) => text.push(c),
            (true, '"') => in_quotes = !in_quotes,
            (true, '>') if !in_quotes => in_tag = false,
            (true, _) => {}
        }
    }
    let decoded = text
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
        .replace("&quot;", "\"")
        .replace("&amp;", "&");
    decoded.split_whitespace().collect::<Vec<_>>().join(" ")
}

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

    fn html(raw: &str) -> Option<String> {
        Description::new(raw)
            .unwrap()
            .map(|d| d.as_html().to_string())
    }

    #[test]
    fn keeps_formatting() {
        assert_eq!(
            html("<p>Soft <strong>cotton</strong></p><ul><li>Wash cold</li></ul>"),
            Some("<p>Soft <strong>cotton</strong></p><ul><li>Wash cold</li></ul>".to_string())
        );
    }

    #[test]
    fn removes_scripts_and_event_attributes() {
        assert_eq!(
            html(r#"<p onclick="steal()">Hi</p><script>steal()</script>"#),
            Some("<p>Hi</p>".to_string())
        );
    }

    #[test]
    fn removes_javascript_links() {
        assert_eq!(
            html(r#"<a href="javascript:steal()">x</a>"#),
            Some(r#"<a rel="noopener noreferrer">x</a>"#.to_string())
        );
    }

    #[test]
    fn sanitising_twice_changes_nothing() {
        let once = html(r#"<p>A &amp; B <a href="https://x.test/?a=1&b=2">link</a></p>"#).unwrap();
        assert_eq!(html(&once), Some(once));
    }

    #[test]
    fn blank_or_only_removed_markup_is_none() {
        assert_eq!(html("   "), None);
        assert_eq!(html("<p> </p>"), None);
        assert_eq!(html("<script>x()</script>"), None);
    }

    #[test]
    fn an_image_alone_is_a_description() {
        assert_eq!(
            html(r#"<img src="https://x.test/a.png">"#),
            Some(r#"<img src="https://x.test/a.png">"#.to_string())
        );
    }

    #[test]
    fn length_is_capped_at_the_limit() {
        let at_limit = "a".repeat(Description::MAX_LEN);
        assert!(Description::new(&at_limit).is_ok());
        assert_eq!(
            Description::new(&format!("{at_limit}a")),
            Err(ProductError::DescriptionTooLong {
                max: Description::MAX_LEN
            })
        );
    }

    #[test]
    fn text_drops_markup_and_decodes_entities() {
        let d = Description::new(
            r#"<p>Fish &amp; chips</p><p>a&lt;b <a href="https://x.test/?q=1>2">here</a></p>"#,
        )
        .unwrap()
        .unwrap();
        assert_eq!(d.text(), "Fish & chips a<b here");
    }

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
