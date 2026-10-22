//! Rendering a template: minijinja, the same natively and on Workers. One `Environment` per
//! render, holding the layout and the template's sources, so a template can only reach the
//! layout and itself (no files, no I/O).

use minijinja::{
    AutoEscape, Environment, ErrorKind, Output, State, UndefinedBehavior, escape_formatter,
    value::Value,
};
use serde::Serialize;

use crate::{RenderError, TemplatePart, TemplateProblem, TemplateSources};

/// The instructions one render of one part may run (minijinja's fuel, mostly one per
/// instruction): a confirmation with a hundred lines needs a few thousand. It bounds
/// what a template's loops can burn, which matters most on Workers, where the CPU time
/// of a request is capped.
pub const FUEL: u64 = 250_000;

/// A rendered email.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    /// One line: trimmed, newlines and runs of whitespace made single spaces.
    pub subject: String,
    pub html: String,
    /// The text template's output, or the HTML's text when the template has none.
    pub text: String,
}

/// Renders `sources` against `context`, its HTML able to `{% extends "layout" %}` the
/// `layout` source. HTML is escaped in the HTML body and the layout only (a subject or
/// text body isn't HTML); undefined is strict, so a misspelled variable is an error
/// rather than an empty string; each part gets `FUEL`.
pub fn render<C: Serialize + ?Sized>(
    layout: &str,
    sources: &TemplateSources,
    context: &C,
) -> Result<Rendered, RenderError> {
    let mut env = environment();
    let mut add = |part: TemplatePart, source| {
        env.add_template(part.as_str(), source)
            .map_err(|e| render_error(&e, part))
    };
    add(TemplatePart::Layout, layout)?;
    add(TemplatePart::Subject, sources.subject())?;
    add(TemplatePart::Html, sources.html())?;
    if let Some(text) = sources.text() {
        add(TemplatePart::Text, text)?;
    }
    let context = Value::from(minijinja::value::Serde(context));
    let source = |part: TemplatePart| match part {
        TemplatePart::Layout => Some(layout),
        TemplatePart::Subject => Some(sources.subject()),
        TemplatePart::Html => Some(sources.html()),
        TemplatePart::Text => sources.text(),
    };
    let run = |part: TemplatePart| {
        env.get_template(part.as_str())
            .and_then(|template| template.render(context.clone()))
            .map_err(|e| with_line(render_error(&e, part), &e, source))
    };
    let subject = one_line(&run(TemplatePart::Subject)?);
    let html = run(TemplatePart::Html)?;
    let text = match sources.text() {
        Some(_) => run(TemplatePart::Text)?.trim().to_string(),
        None => html_to_text(&html),
    };
    Ok(Rendered {
        subject,
        html,
        text,
    })
}

/// Whether `source` parses as Jinja, as `part` (`TemplateSources::new` and `restore`).
pub(crate) fn parse(part: TemplatePart, source: &str) -> Result<(), TemplateProblem> {
    environment()
        .add_template(part.as_str(), source)
        .map_err(|e| problem(&e, part))
}

fn environment<'source>() -> Environment<'source> {
    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    env.set_fuel(Some(FUEL));
    env.set_auto_escape_callback(|name| {
        if name == TemplatePart::Html.as_str() || name == TemplatePart::Layout.as_str() {
            AutoEscape::Html
        } else {
            AutoEscape::None
        }
    });
    env.set_formatter(format_value);
    env
}

/// How a value is printed: in HTML, with `&`, `<`, `>`, `"` and `'` escaped, which is
/// all an element's text or a quoted attribute needs. minijinja's own HTML escaping also
/// writes `/` as `&#x2f;`, which leaves every link in an email reading
/// `https:&#x2f;&#x2f;…` in its source (browsers decode it, but people read the source in
/// the editor and the preview). Strings only: numbers, booleans, safe values (`|safe`,
/// the blocks a template fills) and everything outside HTML print as minijinja would.
fn format_value(
    out: &mut Output,
    state: &mut State,
    value: &Value,
) -> Result<(), minijinja::Error> {
    match value.as_str() {
        Some(text) if *state.auto_escape() == AutoEscape::Html && !value.is_safe() => {
            write_html_escaped(out, text).map_err(minijinja::Error::from)
        }
        _ => escape_formatter(out, state, value),
    }
}

/// `text` with the five characters HTML gives meaning to written as entities.
fn write_html_escaped(out: &mut impl std::fmt::Write, text: &str) -> std::fmt::Result {
    let mut rest = text;
    while let Some(at) = rest.find(['&', '<', '>', '"', '\'']) {
        out.write_str(&rest[..at])?;
        out.write_str(match rest.as_bytes()[at] {
            b'&' => "&amp;",
            b'<' => "&lt;",
            b'>' => "&gt;",
            b'"' => "&quot;",
            _ => "&#x27;",
        })?;
        rest = &rest[at + 1..];
    }
    out.write_str(rest)
}

/// The part an error happened in: the template minijinja names (a block of the HTML
/// rendered through the layout is the HTML's), else the one being rendered.
fn part_of(e: &minijinja::Error, rendering: TemplatePart) -> TemplatePart {
    [
        TemplatePart::Subject,
        TemplatePart::Html,
        TemplatePart::Text,
        TemplatePart::Layout,
    ]
    .into_iter()
    .find(|part| e.name() == Some(part.as_str()))
    .unwrap_or(rendering)
}

fn problem(e: &minijinja::Error, rendering: TemplatePart) -> TemplateProblem {
    let message = match e.detail() {
        Some(detail) => format!("{}: {detail}", e.kind()),
        None => e.kind().to_string(),
    };
    TemplateProblem {
        part: part_of(e, rendering),
        line: e.line(),
        message,
    }
}

/// The longest quote of the failing line in a problem's message.
const QUOTE_CHARS: usize = 100;

/// Quotes the line an undefined value is on: minijinja says only "undefined value",
/// and the line is what lets a merchant spot `customer.nmae`.
fn with_line<'a>(
    error: RenderError,
    e: &minijinja::Error,
    source: impl Fn(TemplatePart) -> Option<&'a str>,
) -> RenderError {
    let RenderError::Invalid(mut problem) = error else {
        return error;
    };
    if e.kind() == ErrorKind::UndefinedError
        && let Some(line) = problem.line
        && let Some(text) = source(problem.part).and_then(|s| s.lines().nth(line - 1))
    {
        let text = text.trim();
        let quote: String = text.chars().take(QUOTE_CHARS).collect();
        let more = if quote.len() < text.len() { "…" } else { "" };
        problem.message = format!("{} in “{quote}{more}”", problem.message);
    }
    RenderError::Invalid(problem)
}

fn render_error(e: &minijinja::Error, rendering: TemplatePart) -> RenderError {
    if e.kind() == ErrorKind::OutOfFuel {
        return RenderError::TooComplex(part_of(e, rendering));
    }
    RenderError::Invalid(problem(e, rendering))
}

/// A subject line: headers are one line, so newlines from the template's own layout
/// (a `{% if %}` on its own line) mustn't reach it.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Plain text from an email's HTML, for clients that show no HTML and for spam filters
/// that compare the two. A small converter rather than a dependency: email HTML is
/// tables and inline styles, and all this needs is readable text. Links are kept as
/// `text (url)`; what isn't shown (`<head>`, `<style>`, comments, elements styled
/// `display:none`, such as a preheader) is dropped; blocks become line breaks, runs of
/// whitespace one space, and at most one blank line separates paragraphs.
pub fn html_to_text(html: &str) -> String {
    let mut out = TextOut::default();
    // The element whose content is dropped, by name, with how many of the same name are
    // open inside it, so its own end tag is the one that ends the skipping.
    let mut skipping: Option<(String, usize)> = None;
    // Open links: their href and where their text starts in `out`.
    let mut links: Vec<(Option<String>, usize)> = Vec::new();
    let mut rest = html;
    while !rest.is_empty() {
        let Some(lt) = rest.find('<') else {
            if skipping.is_none() {
                out.text(&decode_entities(rest));
            }
            break;
        };
        if skipping.is_none() {
            out.text(&decode_entities(&rest[..lt]));
        }
        rest = &rest[lt..];
        if let Some(after) = rest.strip_prefix("<!--") {
            rest = after.find("-->").map_or("", |end| &after[end + 3..]);
            continue;
        }
        let Some(tag) = Tag::parse(rest) else {
            // A `<` that starts no tag is text.
            if skipping.is_none() {
                out.text("<");
            }
            rest = &rest[1..];
            continue;
        };
        rest = &rest[tag.len..];
        if let Some((name, depth)) = &mut skipping {
            if tag.name == *name && !tag.self_closing && !is_void(&tag.name) {
                if tag.closing {
                    if *depth == 0 {
                        skipping = None;
                    } else {
                        *depth -= 1;
                    }
                } else {
                    *depth += 1;
                }
            }
            continue;
        }
        if tag.closing {
            match tag.name.as_str() {
                "a" => {
                    if let Some((href, start)) = links.pop() {
                        out.link_end(href.as_deref(), start);
                    }
                }
                name => out.block(name),
            }
            continue;
        }
        let hidden = matches!(
            tag.name.as_str(),
            "head" | "style" | "script" | "title" | "template"
        ) || tag.hidden();
        if hidden && !tag.self_closing && !is_void(&tag.name) {
            skipping = Some((tag.name.clone(), 0));
            continue;
        }
        match tag.name.as_str() {
            "br" => out.line_break(),
            "a" => links.push((tag.attr("href").map(|h| decode_entities(&h)), out.len())),
            "li" => {
                out.block("li");
                out.text("- ");
            }
            "td" | "th" => out.space(),
            name => out.block(name),
        }
    }
    out.finish()
}

/// Void elements have no end tag.
fn is_void(name: &str) -> bool {
    matches!(
        name,
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "source"
            | "track"
            | "wbr"
    )
}

/// The text being built: whitespace collapsed as HTML does, line breaks where blocks
/// end.
#[derive(Default)]
struct TextOut {
    out: String,
    /// Whitespace seen since the last text: one space, if more text follows on the line.
    space: bool,
}

impl TextOut {
    fn len(&self) -> usize {
        self.out.len()
    }

    fn text(&mut self, text: &str) {
        for c in text.chars() {
            if c.is_whitespace() && c != '\u{a0}' {
                self.space = true;
                continue;
            }
            if self.space && !self.out.is_empty() && !self.out.ends_with(['\n', ' ']) {
                self.out.push(' ');
            }
            self.space = false;
            // A non-breaking space is a space in plain text.
            self.out.push(if c == '\u{a0}' { ' ' } else { c });
        }
    }

    fn space(&mut self) {
        self.space = true;
    }

    fn trim_end_spaces(&mut self) {
        let kept = self.out.trim_end_matches([' ', '\t']).len();
        self.out.truncate(kept);
    }

    fn line_break(&mut self) {
        self.trim_end_spaces();
        self.out.push('\n');
        self.space = false;
    }

    /// Ends the line, and leaves a blank line after it when `blank` (paragraphs).
    fn break_lines(&mut self, blank: bool) {
        self.trim_end_spaces();
        self.space = false;
        if self.out.is_empty() {
            return;
        }
        let wanted = if blank { 2 } else { 1 };
        let have = self.out.len() - self.out.trim_end_matches('\n').len();
        for _ in have..wanted {
            self.out.push('\n');
        }
    }

    /// An element's start or end: block elements break the line.
    fn block(&mut self, name: &str) {
        match name {
            "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "ul" | "ol" | "blockquote" | "hr"
            | "pre" => self.break_lines(true),
            "div" | "tr" | "table" | "tbody" | "thead" | "tfoot" | "li" | "section" | "header"
            | "footer" | "article" | "main" | "nav" | "body" | "html" | "dl" | "dt" | "dd"
            | "center" => self.break_lines(false),
            _ => {}
        }
    }

    /// A link's end: its URL after its text, unless that says nothing more (the text is
    /// the URL, an address and its `mailto:`, an anchor on the page).
    fn link_end(&mut self, href: Option<&str>, start: usize) {
        let Some(href) = href.map(str::trim).filter(|h| !h.is_empty()) else {
            return;
        };
        if href.starts_with('#') || href.starts_with("javascript:") {
            return;
        }
        let text = self.out.get(start..).unwrap_or("").trim().to_string();
        let bare = href.strip_prefix("mailto:").unwrap_or(href);
        if text == href || text == bare {
            return;
        }
        if text.is_empty() {
            self.text(bare);
        } else {
            self.text(&format!(" ({bare})"));
        }
    }

    fn finish(self) -> String {
        let mut text = String::with_capacity(self.out.len());
        let mut blank_lines = 0;
        for line in self.out.lines().map(str::trim) {
            if line.is_empty() {
                blank_lines += 1;
                continue;
            }
            if !text.is_empty() {
                text.push_str(if blank_lines > 0 { "\n\n" } else { "\n" });
            }
            blank_lines = 0;
            text.push_str(line);
        }
        text
    }
}

/// One start or end tag.
struct Tag {
    /// Lowercase.
    name: String,
    closing: bool,
    self_closing: bool,
    /// The attributes' raw text.
    attrs: String,
    /// Bytes from `<` through `>`.
    len: usize,
}

impl Tag {
    /// The tag at the start of `s` (which starts with `<`); `None` when it isn't one
    /// (`a < b`, a `<!DOCTYPE …>` is one, with no name that matters).
    fn parse(s: &str) -> Option<Tag> {
        let body = &s[1..];
        let (closing, body) = match body.strip_prefix('/') {
            Some(body) => (true, body),
            None => (false, body),
        };
        let first = body.chars().next()?;
        if !(first.is_ascii_alphabetic() || first == '!' || first == '?') {
            return None;
        }
        // The end: the first `>` outside quotes.
        let mut quote = None;
        let mut end = None;
        for (i, c) in body.char_indices() {
            match (quote, c) {
                (Some(q), c) if c == q => quote = None,
                (Some(_), _) => {}
                (None, '"' | '\'') => quote = Some(c),
                (None, '>') => {
                    end = Some(i);
                    break;
                }
                _ => {}
            }
        }
        let end = end?;
        let inner = &body[..end];
        let name_len = inner
            .find(|c: char| c.is_whitespace() || c == '/')
            .unwrap_or(inner.len());
        let name = inner[..name_len].to_ascii_lowercase();
        let attrs = inner[name_len..].trim();
        let self_closing = attrs.ends_with('/');
        Some(Tag {
            name,
            closing,
            self_closing,
            attrs: attrs.trim_end_matches('/').to_string(),
            len: 1 + usize::from(closing) + end + 1,
        })
    }

    /// The value of attribute `wanted`, unquoted (entities still encoded).
    fn attr(&self, wanted: &str) -> Option<String> {
        let mut rest = self.attrs.as_str();
        loop {
            rest = rest.trim_start();
            if rest.is_empty() {
                return None;
            }
            let name_len = rest
                .find(|c: char| c.is_whitespace() || c == '=')
                .unwrap_or(rest.len());
            let name = rest[..name_len].to_ascii_lowercase();
            rest = rest[name_len..].trim_start();
            let value = if let Some(after) = rest.strip_prefix('=') {
                let after = after.trim_start();
                let (value, remaining) = match after.chars().next() {
                    Some(q @ ('"' | '\'')) => {
                        let inner = &after[1..];
                        let close = inner.find(q).unwrap_or(inner.len());
                        (&inner[..close], inner.get(close + 1..).unwrap_or(""))
                    }
                    _ => {
                        let close = after.find(char::is_whitespace).unwrap_or(after.len());
                        (&after[..close], &after[close..])
                    }
                };
                rest = remaining;
                Some(value)
            } else {
                None
            };
            if name == wanted {
                return Some(value.unwrap_or("").to_string());
            }
        }
    }

    /// Styled out of sight: `display:none` (a preheader), or `hidden`.
    fn hidden(&self) -> bool {
        let style: String = self
            .attr("style")
            .unwrap_or_default()
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
            .to_ascii_lowercase();
        style.contains("display:none") || self.attr("hidden").is_some()
    }
}

/// The entities email HTML uses, by name, and every numeric one.
fn decode_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let decoded = rest[1..]
            .find(';')
            .filter(|&end| end <= 10)
            .and_then(|end| entity(&rest[1..=end]).map(|c| (c, end + 2)));
        match decoded {
            Some((c, len)) => {
                out.push(c);
                rest = &rest[len..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn entity(name: &str) -> Option<char> {
    if let Some(number) = name.strip_prefix('#') {
        let code = match number.strip_prefix(['x', 'X']) {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => number.parse().ok()?,
        };
        return char::from_u32(code);
    }
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{a0}',
        "ndash" => '–',
        "mdash" => '—',
        "hellip" => '…',
        "lsquo" => '‘',
        "rsquo" => '’',
        "ldquo" => '“',
        "rdquo" => '”',
        "bull" => '•',
        "middot" => '·',
        "copy" => '©',
        "reg" => '®',
        "trade" => '™',
        "euro" => '€',
        "pound" => '£',
        "times" => '×',
        "zwnj" => '\u{200c}',
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::TemplateKey;

    const LAYOUT: &str = "<html><body>{% block content %}{% endblock %}</body></html>";

    fn sources(subject: &str, html: &str, text: Option<&str>) -> TemplateSources {
        TemplateSources::new(
            TemplateKey::OrderConfirmation,
            subject.to_string(),
            html.to_string(),
            text.map(str::to_string),
        )
        .unwrap()
    }

    #[test]
    fn html_is_escaped_but_subject_and_text_are_not() {
        let rendered = render(
            LAYOUT,
            &sources("Hi {{ name }}", "<p>{{ name }}</p>", Some("Hi {{ name }}")),
            &json!({"name": "Tom & <Jerry>"}),
        )
        .unwrap();
        assert_eq!(rendered.subject, "Hi Tom & <Jerry>");
        assert_eq!(rendered.html, "<p>Tom &amp; &lt;Jerry&gt;</p>");
        assert_eq!(rendered.text, "Hi Tom & <Jerry>");
    }

    #[test]
    fn a_url_in_an_attribute_keeps_its_slashes() {
        let rendered = render(
            LAYOUT,
            &sources("Hi", r#"<a href="{{ url }}">{{ label }}</a>"#, None),
            &json!({"url": "https://shop.example/orders/1?a=1&b='2'", "label": "\"Go\" / see"}),
        )
        .unwrap();
        assert_eq!(
            rendered.html,
            r#"<a href="https://shop.example/orders/1?a=1&amp;b=&#x27;2&#x27;">&quot;Go&quot; / see</a>"#
        );
    }

    #[test]
    fn numbers_and_safe_values_print_as_they_are() {
        let rendered = render(
            LAYOUT,
            &sources("Hi", "{{ n }} {{ yes }} {{ html|safe }}", None),
            &json!({"n": 12, "yes": true, "html": "<b>/</b>"}),
        )
        .unwrap();
        assert_eq!(rendered.html, "12 True <b>/</b>");
    }

    #[test]
    fn the_html_extends_the_layout() {
        let rendered = render(
            LAYOUT,
            &sources(
                "Hi",
                r#"{% extends "layout" %}{% block content %}<p>{{ shop }}</p>{% endblock %}"#,
                None,
            ),
            &json!({"shop": "Kafa & co"}),
        )
        .unwrap();
        assert_eq!(
            rendered.html,
            "<html><body><p>Kafa &amp; co</p></body></html>"
        );
    }

    #[test]
    fn an_undefined_variable_is_an_error_with_its_line() {
        let result = render(
            LAYOUT,
            &sources("Hi", "<p>\n{{ order.nubmer }}\n</p>", None),
            &json!({"order": {"number": "1001"}}),
        );
        let Err(RenderError::Invalid(problem)) = result else {
            panic!("expected template_invalid, got {result:?}");
        };
        assert_eq!(problem.part, TemplatePart::Html);
        assert_eq!(problem.line, Some(2));
        assert_eq!(
            problem.message, "undefined value in “{{ order.nubmer }}”",
            "the line is quoted, so the misspelling shows"
        );
    }

    #[test]
    fn a_null_is_defined_and_falsy() {
        let rendered = render(
            LAYOUT,
            &sources(
                "Hi",
                "{% if order.url %}link{% else %}none{% endif %}",
                None,
            ),
            &json!({"order": {"url": null}}),
        )
        .unwrap();
        assert_eq!(rendered.html, "none");
    }

    #[test]
    fn an_error_in_the_layout_names_the_layout() {
        let result = render(
            "<html>{{ brand.logo_url }}{% block content %}{% endblock %}</html>",
            &sources(
                "Hi",
                r#"{% extends "layout" %}{% block content %}x{% endblock %}"#,
                None,
            ),
            &json!({}),
        );
        assert!(
            matches!(&result, Err(RenderError::Invalid(p)) if p.part == TemplatePart::Layout),
            "{result:?}"
        );
    }

    #[test]
    fn a_runaway_loop_runs_out_of_fuel() {
        let result = render(
            LAYOUT,
            &sources(
                "Hi",
                "{% for a in range(1000) %}{% for b in range(1000) %}x{% endfor %}{% endfor %}",
                None,
            ),
            &json!({}),
        );
        assert_eq!(result, Err(RenderError::TooComplex(TemplatePart::Html)));
    }

    #[test]
    fn the_subject_is_one_line() {
        let rendered = render(
            LAYOUT,
            &sources(
                "\n  Order {{ n }}\n{% if true %}\n  shipped\n{% endif %}\n",
                "<p>x</p>",
                None,
            ),
            &json!({"n": 7}),
        )
        .unwrap();
        assert_eq!(rendered.subject, "Order 7 shipped");
    }

    #[test]
    fn without_a_text_body_the_text_comes_from_the_html() {
        let rendered = render(
            LAYOUT,
            &sources("Hi", "<h1>Thanks</h1><p>Order <b>1001</b></p>", None),
            &json!({}),
        )
        .unwrap();
        assert_eq!(rendered.text, "Thanks\n\nOrder 1001");
    }

    #[test]
    fn text_keeps_links_as_text_and_url() {
        assert_eq!(
            html_to_text(r#"<p>See <a href="https://s.com/o?a=1&amp;b=2">your order</a>.</p>"#),
            "See your order (https://s.com/o?a=1&b=2)."
        );
        assert_eq!(
            html_to_text(r#"<a href="https://s.com">https://s.com</a>"#),
            "https://s.com"
        );
        assert_eq!(
            html_to_text(r#"Write to <a href="mailto:help@s.com">help@s.com</a>"#),
            "Write to help@s.com"
        );
        assert_eq!(html_to_text(r##"<a href="#top">Top</a>"##), "Top");
    }

    #[test]
    fn text_drops_what_isnt_shown() {
        let html = r#"<!doctype html><html><head><title>T</title><style>p{color:red}</style></head>
            <body><!--[if mso]>x<![endif]--><div style="display: none; max-height:0">Preheader <div>inner</div> still</div>
            <p>Shown</p><img src="logo.png" alt="Logo"></body></html>"#;
        assert_eq!(html_to_text(html), "Shown");
    }

    #[test]
    fn text_breaks_lines_at_blocks_and_rows() {
        let html = "<table><tr><td>Mug</td><td>2</td><td>&euro;10.00</td></tr>\
                    <tr><td>Cup</td><td>1</td><td>&#8364;5.00</td></tr></table>\
                    <p>Line one<br>Line&nbsp;two</p><ul><li>a</li><li>b</li></ul>";
        assert_eq!(
            html_to_text(html),
            "Mug 2 €10.00\nCup 1 €5.00\n\nLine one\nLine two\n\n- a\n- b"
        );
    }

    #[test]
    fn text_collapses_whitespace_and_keeps_stray_symbols() {
        assert_eq!(
            html_to_text("  <p>a\n   b  &unknown; 1 < 2 &amp</p>\n\n\n<p>c</p>  "),
            "a b &unknown; 1 < 2 &amp\n\nc"
        );
    }
}
