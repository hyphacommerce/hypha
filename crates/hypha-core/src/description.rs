//! Rich descriptions (design note 0019, Q1; 0022, CA9): sanitised HTML, shared by
//! products and categories.

use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DescriptionError {
    #[error("description is longer than {max} bytes")]
    TooLong { max: usize },
}

impl crate::ErrorCode for DescriptionError {
    fn code(&self) -> &'static str {
        match self {
            DescriptionError::TooLong { .. } => "description_too_long",
        }
    }
}

/// A rich description (products, categories): HTML, sanitised when it's made, so what's stored is always
/// safe to render (design note 0019, Q1). An allow-list keeps paragraphs, headings,
/// lists, emphasis, links, tables and images, and drops scripts, styles, event
/// attributes and unsafe URLs (`ammonia`'s defaults).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Description(String);

impl Description {
    /// Longest description, in bytes of HTML, both as sent and as sanitised (escaping
    /// can make it longer: `<` becomes `&lt;`, links gain a `rel`).
    pub const MAX_LEN: usize = 64 * 1024;

    /// Sanitises `html`. `None` when nothing is left (blank, or only markup that was
    /// removed). The limit applies to what's stored, so a description accepted here
    /// always loads again (`stored`).
    pub fn new(html: &str) -> Result<Option<Self>, DescriptionError> {
        if html.len() > Self::MAX_LEN {
            return Err(DescriptionError::TooLong { max: Self::MAX_LEN });
        }
        let clean = ammonia::clean(html);
        let clean = clean.trim();
        if clean.len() > Self::MAX_LEN {
            return Err(DescriptionError::TooLong { max: Self::MAX_LEN });
        }
        if text_of(clean).is_empty() && !clean.contains("<img") {
            return Ok(None);
        }
        Ok(Some(Self(clean.to_string())))
    }

    /// A description as `new` stored it, for `Product::restore`. Sanitising is a
    /// transformation, not a check, and it's done on the way in: running it again on
    /// every load (carts, checkout, search) would parse up to 64 KiB of HTML per product
    /// for nothing. Only the length is checked again. Storage is trusted to hand back
    /// what it was given, as for every other field.
    pub fn stored(html: String) -> Result<Option<Self>, DescriptionError> {
        if html.len() > Self::MAX_LEN {
            return Err(DescriptionError::TooLong { max: Self::MAX_LEN });
        }
        Ok((!html.is_empty()).then_some(Self(html)))
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

#[cfg(test)]
mod tests {
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
    fn the_limit_applies_to_the_sanitised_html_too() {
        // 30 000 `<` are 30 000 bytes sent and 120 000 stored (`&lt;`).
        assert_eq!(
            Description::new(&"<".repeat(30_000)),
            Err(DescriptionError::TooLong {
                max: Description::MAX_LEN
            })
        );
    }

    #[test]
    fn a_stored_description_is_taken_as_stored() {
        let d = Description::new("<p>Hi</p>").unwrap().unwrap();
        assert_eq!(Description::stored(d.as_html().to_string()), Ok(Some(d)));
        assert_eq!(Description::stored(String::new()), Ok(None));
        assert_eq!(
            Description::stored("a".repeat(Description::MAX_LEN + 1)),
            Err(DescriptionError::TooLong {
                max: Description::MAX_LEN
            })
        );
    }

    #[test]
    fn length_is_capped_at_the_limit() {
        let at_limit = "a".repeat(Description::MAX_LEN);
        assert!(Description::new(&at_limit).is_ok());
        assert_eq!(
            Description::new(&format!("{at_limit}a")),
            Err(DescriptionError::TooLong {
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
}
