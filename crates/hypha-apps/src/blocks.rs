//! The declarative block format, version 1: the UI tree an app's block URL answers, which
//! Hypha checks before handing it to an admin, so every admin renders the same checked
//! shapes. The rules are `packages/app-blocks`' (`schema/block.v1.json` and
//! `src/validate.ts`), ported: the tests below run that package's fixtures, so the two
//! can't drift.
//!
//! Elements of a type version 1 doesn't know are warnings, not errors (a newer app on
//! an older Hypha): hosts skip them.

use serde_json::{Map, Value};

/// Trees nested deeper than this are refused.
pub const MAX_BLOCK_DEPTH: usize = 16;

/// The element types version 1 knows.
pub const ELEMENT_TYPES: [&str; 15] = [
    "card", "stack", "row", "text", "heading", "badge", "link", "button", "divider", "image",
    "list", "table", "keyvalue", "form", "banner",
];

/// One problem: where (`root.children[1].label`) and what.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockProblem {
    pub path: String,
    pub message: String,
}

/// What checking a tree found: no `errors` means it's valid.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BlockCheck {
    pub errors: Vec<BlockProblem>,
    /// Elements of types this version doesn't know (skipped by hosts).
    pub warnings: Vec<BlockProblem>,
}

impl BlockCheck {
    /// Why the tree is refused, naming its first problem; `None` when it's valid.
    pub fn problem(&self) -> Option<String> {
        let first = self.errors.first()?;
        Some(format!(
            "not a block v1: {}: {} ({} problem(s))",
            first.path,
            first.message,
            self.errors.len()
        ))
    }
}

/// Checks `value` against version 1's rules, finding every problem.
pub fn check_block(value: &Value) -> BlockCheck {
    let mut c = Checker::default();
    if let Some(top) = c.object(value, "$") {
        c.keys(top, "$", &["version", "root"]);
        if top.get("version").and_then(Value::as_f64) != Some(1.0) {
            c.error("$.version", "must be 1");
        }
        match top.get("root") {
            None => c.error("$.root", "is required"),
            Some(root) => c.element(root, "root", 1),
        }
    }
    BlockCheck {
        errors: c.errors,
        warnings: c.warnings,
    }
}

/// Lengths as JavaScript counts them (UTF-16 code units), so the limits mean the same
/// here and in the package.
fn js_len(s: &str) -> usize {
    s.encode_utf16().count()
}

fn is_action_id(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && js_len(s) <= 64
        && chars
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '.' | '-'))
}

/// A path within the app's page: one `/` first, not two.
fn is_path(s: &str) -> bool {
    s.starts_with('/') && !s.starts_with("//")
}

fn is_column_key(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn is_field_name(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && s.len() <= 64
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// A check on a string, and the regular expression it stands for (for messages).
type Pattern = (fn(&str) -> bool, &'static str);

/// What a string must be.
struct Rules {
    required: bool,
    min: usize,
    max: usize,
    pattern: Option<Pattern>,
}

const ACTION: Option<Pattern> = Some((is_action_id, "^[a-z][a-z0-9_.-]{0,63}$"));

#[derive(Default)]
struct Checker {
    errors: Vec<BlockProblem>,
    warnings: Vec<BlockProblem>,
}

impl Checker {
    fn error(&mut self, path: &str, message: impl Into<String>) {
        self.errors.push(BlockProblem {
            path: path.to_string(),
            message: message.into(),
        });
    }

    fn object<'v>(&mut self, value: &'v Value, path: &str) -> Option<&'v Map<String, Value>> {
        match value.as_object() {
            Some(map) => Some(map),
            None => {
                self.error(path, "must be an object");
                None
            }
        }
    }

    fn keys(&mut self, value: &Map<String, Value>, path: &str, allowed: &[&str]) {
        for key in value.keys() {
            if !allowed.contains(&key.as_str()) {
                self.error(&format!("{path}.{key}"), "is not allowed here");
            }
        }
    }

    fn string(&mut self, value: &Map<String, Value>, key: &str, path: &str, rules: Rules) {
        let at = format!("{path}.{key}");
        let Some(v) = value.get(key) else {
            if rules.required {
                self.error(&at, "is required");
            }
            return;
        };
        let Some(s) = v.as_str() else {
            return self.error(&at, "must be a string");
        };
        let len = js_len(s);
        if len < rules.min {
            self.error(
                &at,
                format!("must have at least {} character(s)", rules.min),
            );
        }
        if len > rules.max {
            self.error(&at, format!("must have at most {} characters", rules.max));
        }
        if let Some((matches, source)) = rules.pattern
            && !matches(s)
        {
            self.error(&at, format!("must match {source}"));
        }
    }

    fn label(&mut self, value: &Map<String, Value>, key: &str, path: &str, required: bool) {
        let rules = Rules {
            required,
            min: 1,
            max: 200,
            pattern: None,
        };
        self.string(value, key, path, rules);
    }

    fn text(&mut self, value: &Map<String, Value>, key: &str, path: &str, required: bool) {
        let rules = Rules {
            required,
            min: 0,
            max: 2000,
            pattern: None,
        };
        self.string(value, key, path, rules);
    }

    fn https(&mut self, value: &Map<String, Value>, key: &str, path: &str) {
        let rules = Rules {
            required: true,
            min: 0,
            max: 2048,
            pattern: Some((|s| s.starts_with("https://"), "^https://")),
        };
        self.string(value, key, path, rules);
    }

    fn one_of_str(
        &mut self,
        value: &Map<String, Value>,
        key: &str,
        path: &str,
        options: &[&str],
        required: bool,
    ) {
        let at = format!("{path}.{key}");
        match value.get(key) {
            None if required => self.error(&at, "is required"),
            None => {}
            Some(v) if v.as_str().is_some_and(|s| options.contains(&s)) => {}
            Some(_) => {
                let list: Vec<String> = options.iter().map(|o| format!("{o:?}")).collect();
                self.error(&at, format!("must be one of {}", list.join(", ")));
            }
        }
    }

    fn one_of_num(&mut self, value: &Map<String, Value>, key: &str, path: &str, options: &[f64]) {
        match value.get(key) {
            None => {}
            Some(v) if v.as_f64().is_some_and(|n| options.contains(&n)) => {}
            Some(_) => {
                let list: Vec<String> = options.iter().map(|o| o.to_string()).collect();
                self.error(
                    &format!("{path}.{key}"),
                    format!("must be one of {}", list.join(", ")),
                );
            }
        }
    }

    fn boolean(&mut self, value: &Map<String, Value>, key: &str, path: &str) {
        if value.get(key).is_some_and(|v| !v.is_boolean()) {
            self.error(&format!("{path}.{key}"), "must be a boolean");
        }
    }

    fn number(
        &mut self,
        value: &Map<String, Value>,
        key: &str,
        path: &str,
        integer: Option<(f64, f64)>,
    ) {
        let Some(v) = value.get(key) else { return };
        let at = format!("{path}.{key}");
        let Some(n) = v.as_f64() else {
            return self.error(&at, "must be a number");
        };
        if let Some((min, max)) = integer
            && (n.fract() != 0.0 || n < min || n > max)
        {
            self.error(&at, format!("must be an integer from {min} to {max}"));
        }
    }

    fn array<'v>(
        &mut self,
        value: &'v Map<String, Value>,
        key: &str,
        path: &str,
        min: usize,
        max: usize,
    ) -> &'v [Value] {
        let at = format!("{path}.{key}");
        let Some(v) = value.get(key) else {
            self.error(&at, "is required");
            return &[];
        };
        let Some(items) = v.as_array() else {
            self.error(&at, "must be an array");
            return &[];
        };
        if items.len() < min {
            self.error(&at, format!("must have at least {min} item(s)"));
        }
        if items.len() > max {
            self.error(&at, format!("must have at most {max} items"));
        }
        items
    }

    fn children(&mut self, value: &Map<String, Value>, path: &str, depth: usize) {
        for (i, child) in self
            .array(value, "children", path, 0, 100)
            .iter()
            .enumerate()
        {
            self.element(child, &format!("{path}.children[{i}]"), depth + 1);
        }
    }

    fn element(&mut self, value: &Value, path: &str, depth: usize) {
        let Some(v) = self.object(value, path) else {
            return;
        };
        if depth > MAX_BLOCK_DEPTH {
            return self.error(path, format!("is nested deeper than {MAX_BLOCK_DEPTH}"));
        }
        let ty = match v.get("type").and_then(Value::as_str) {
            Some(ty) if !ty.is_empty() => ty,
            _ => return self.error(&format!("{path}.type"), "must be a non-empty string"),
        };
        if ty == "field" {
            return self.error(path, "a field belongs in a form's fields");
        }
        if !ELEMENT_TYPES.contains(&ty) {
            self.warnings.push(BlockProblem {
                path: path.to_string(),
                message: format!("unknown element type {ty:?}: skipped"),
            });
            return;
        }
        let allow = |c: &mut Self, keys: &[&str]| {
            let mut all = vec!["type"];
            all.extend_from_slice(keys);
            c.keys(v, path, &all);
        };
        let gaps = [0.0, 1.0, 2.0, 3.0, 4.0];
        match ty {
            "card" => {
                allow(self, &["title", "children"]);
                self.label(v, "title", path, false);
                self.children(v, path, depth);
            }
            "stack" => {
                allow(self, &["gap", "children"]);
                self.one_of_num(v, "gap", path, &gaps);
                self.children(v, path, depth);
            }
            "row" => {
                allow(self, &["gap", "align", "children"]);
                self.one_of_num(v, "gap", path, &gaps);
                self.one_of_str(
                    v,
                    "align",
                    path,
                    &["start", "center", "end", "between"],
                    false,
                );
                self.children(v, path, depth);
            }
            "text" => {
                allow(self, &["value", "variant", "weight"]);
                self.text(v, "value", path, true);
                self.one_of_str(v, "variant", path, &["body", "supporting", "code"], false);
                self.one_of_str(v, "weight", path, &["normal", "semibold"], false);
            }
            "heading" => {
                allow(self, &["value", "level"]);
                self.label(v, "value", path, true);
                self.one_of_num(v, "level", path, &[2.0, 3.0, 4.0]);
            }
            "badge" => {
                allow(self, &["value", "tone"]);
                let rules = Rules {
                    required: true,
                    min: 1,
                    max: 50,
                    pattern: None,
                };
                self.string(v, "value", path, rules);
                let tones = ["neutral", "info", "success", "warning", "error"];
                self.one_of_str(v, "tone", path, &tones, false);
            }
            "link" => {
                allow(self, &["label", "href", "external"]);
                self.label(v, "label", path, true);
                self.https(v, "href", path);
                self.boolean(v, "external", path);
            }
            "button" => {
                allow(self, &["label", "action", "navigate", "variant"]);
                self.label(v, "label", path, true);
                let action = Rules {
                    required: false,
                    min: 0,
                    max: 64,
                    pattern: ACTION,
                };
                self.string(v, "action", path, action);
                let navigate = Rules {
                    required: false,
                    min: 0,
                    max: 2048,
                    pattern: Some((is_path, "^/(?!/)")),
                };
                self.string(v, "navigate", path, navigate);
                if v.contains_key("action") == v.contains_key("navigate") {
                    self.error(path, "needs exactly one of action and navigate");
                }
                let variants = ["primary", "secondary", "destructive"];
                self.one_of_str(v, "variant", path, &variants, false);
            }
            "divider" => allow(self, &[]),
            "image" => {
                allow(self, &["src", "alt", "width", "height"]);
                self.https(v, "src", path);
                let alt = Rules {
                    required: true,
                    min: 0,
                    max: 300,
                    pattern: None,
                };
                self.string(v, "alt", path, alt);
                self.number(v, "width", path, Some((1.0, 2000.0)));
                self.number(v, "height", path, Some((1.0, 2000.0)));
            }
            "list" => {
                allow(self, &["items"]);
                for (i, item) in self.array(v, "items", path, 0, 100).iter().enumerate() {
                    let at = format!("{path}.items[{i}]");
                    let Some(item) = self.object(item, &at) else {
                        continue;
                    };
                    self.keys(item, &at, &["label", "description"]);
                    self.label(item, "label", &at, true);
                    self.text(item, "description", &at, false);
                }
            }
            "table" => {
                allow(self, &["columns", "rows"]);
                for (i, column) in self.array(v, "columns", path, 1, 12).iter().enumerate() {
                    let at = format!("{path}.columns[{i}]");
                    let Some(column) = self.object(column, &at) else {
                        continue;
                    };
                    self.keys(column, &at, &["key", "header", "align"]);
                    let key = Rules {
                        required: true,
                        min: 0,
                        max: 64,
                        pattern: Some((is_column_key, "^[A-Za-z0-9_]{1,64}$")),
                    };
                    self.string(column, "key", &at, key);
                    self.label(column, "header", &at, true);
                    self.one_of_str(column, "align", &at, &["start", "end"], false);
                }
                for (i, row) in self.array(v, "rows", path, 0, 200).iter().enumerate() {
                    let at = format!("{path}.rows[{i}]");
                    let Some(row) = self.object(row, &at) else {
                        continue;
                    };
                    for (key, cell) in row {
                        if !(cell.is_null() || cell.is_string() || cell.is_number()) {
                            self.error(
                                &format!("{at}.{key}"),
                                "must be a string, a number or null",
                            );
                        }
                    }
                }
            }
            "keyvalue" => {
                allow(self, &["items"]);
                for (i, item) in self.array(v, "items", path, 0, 100).iter().enumerate() {
                    let at = format!("{path}.items[{i}]");
                    let Some(item) = self.object(item, &at) else {
                        continue;
                    };
                    self.keys(item, &at, &["label", "value"]);
                    self.label(item, "label", &at, true);
                    self.text(item, "value", &at, true);
                }
            }
            "form" => {
                allow(self, &["action", "submit_label", "fields"]);
                let action = Rules {
                    required: true,
                    min: 0,
                    max: 64,
                    pattern: ACTION,
                };
                self.string(v, "action", path, action);
                self.label(v, "submit_label", path, false);
                for (i, field) in self.array(v, "fields", path, 1, 50).iter().enumerate() {
                    self.field(field, &format!("{path}.fields[{i}]"));
                }
            }
            "banner" => {
                allow(self, &["tone", "title", "description"]);
                let tones = ["info", "success", "warning", "error"];
                self.one_of_str(v, "tone", path, &tones, true);
                self.label(v, "title", path, true);
                self.text(v, "description", path, false);
            }
            _ => unreachable!("every type in ELEMENT_TYPES has an arm"),
        }
    }

    fn field(&mut self, value: &Value, path: &str) {
        let Some(v) = self.object(value, path) else {
            return;
        };
        if v.get("type").and_then(Value::as_str) != Some("field") {
            self.error(&format!("{path}.type"), "must be \"field\"");
        }
        let inputs = ["text", "number", "select", "checkbox"];
        self.one_of_str(v, "input", path, &inputs, true);
        let name = Rules {
            required: true,
            min: 0,
            max: 64,
            pattern: Some((is_field_name, "^[A-Za-z][A-Za-z0-9_]{0,63}$")),
        };
        self.string(v, "name", path, name);
        self.label(v, "label", path, true);
        self.text(v, "description", path, false);
        self.boolean(v, "required", path);
        let common = [
            "type",
            "input",
            "name",
            "label",
            "description",
            "required",
            "value",
        ];
        let with = |extra: &[&'static str]| {
            let mut keys = common.to_vec();
            keys.extend_from_slice(extra);
            keys
        };
        match v.get("input").and_then(Value::as_str) {
            Some("text") => {
                self.keys(v, path, &with(&["placeholder"]));
                self.text(v, "value", path, false);
                self.label(v, "placeholder", path, false);
            }
            Some("number") => {
                self.keys(v, path, &with(&["min", "max"]));
                self.number(v, "value", path, None);
                self.number(v, "min", path, None);
                self.number(v, "max", path, None);
            }
            Some("select") => {
                self.keys(v, path, &with(&["options"]));
                if v.get("value").is_some_and(|x| !x.is_string()) {
                    self.error(&format!("{path}.value"), "must be a string");
                }
                for (i, option) in self.array(v, "options", path, 1, 100).iter().enumerate() {
                    let at = format!("{path}.options[{i}]");
                    let Some(option) = self.object(option, &at) else {
                        continue;
                    };
                    self.keys(option, &at, &["value", "label"]);
                    let value = Rules {
                        required: true,
                        min: 0,
                        max: 200,
                        pattern: None,
                    };
                    self.string(option, "value", &at, value);
                    self.label(option, "label", &at, true);
                }
            }
            Some("checkbox") => {
                self.keys(v, path, &common);
                self.boolean(v, "value", path);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;

    #[derive(Deserialize)]
    struct Valid {
        name: String,
        tree: Value,
        #[serde(default)]
        warnings: Vec<String>,
    }

    #[derive(Deserialize)]
    struct Invalid {
        name: String,
        tree: Value,
        error: String,
    }

    /// `packages/app-blocks`' fixtures: the package's validator and its JSON Schema
    /// answer them the same way, and so must this.
    const VALID: &str = include_str!("../../../packages/app-blocks/fixtures/valid.json");
    const INVALID: &str = include_str!("../../../packages/app-blocks/fixtures/invalid.json");

    #[test]
    fn the_packages_valid_fixtures_are_valid() {
        let fixtures: Vec<Valid> = serde_json::from_str(VALID).unwrap();
        assert!(!fixtures.is_empty());
        for fixture in fixtures {
            let check = check_block(&fixture.tree);
            assert_eq!(check.errors, vec![], "{}", fixture.name);
            let warned: Vec<String> = check.warnings.into_iter().map(|w| w.path).collect();
            assert_eq!(warned, fixture.warnings, "{}", fixture.name);
        }
    }

    #[test]
    fn the_packages_invalid_fixtures_are_invalid_where_they_say() {
        let fixtures: Vec<Invalid> = serde_json::from_str(INVALID).unwrap();
        assert!(!fixtures.is_empty());
        for fixture in fixtures {
            let check = check_block(&fixture.tree);
            assert!(
                check.errors.iter().any(|e| e.path == fixture.error),
                "{}: expected an error at {}, got {:?}",
                fixture.name,
                fixture.error,
                check.errors
            );
        }
    }

    fn nested(depth: usize) -> Value {
        let mut element = serde_json::json!({ "type": "divider" });
        for _ in 1..depth {
            element = serde_json::json!({ "type": "stack", "children": [element] });
        }
        serde_json::json!({ "version": 1, "root": element })
    }

    #[test]
    fn trees_nest_up_to_the_limit() {
        assert!(check_block(&nested(MAX_BLOCK_DEPTH)).problem().is_none());
        assert!(
            check_block(&nested(MAX_BLOCK_DEPTH + 1))
                .problem()
                .is_some()
        );
    }

    #[test]
    fn the_problem_names_the_first_error_and_counts_them() {
        let tree = serde_json::json!({ "version": 2, "root": { "type": "divider" } });
        assert_eq!(
            check_block(&tree).problem().as_deref(),
            Some("not a block v1: $.version: must be 1 (1 problem(s))")
        );
    }

    #[test]
    fn anything_but_an_object_is_invalid() {
        for value in [Value::Null, "x".into(), 1.into(), Value::Array(vec![])] {
            assert!(check_block(&value).problem().is_some());
        }
    }

    #[test]
    fn lengths_count_as_javascript_does() {
        // One emoji is two UTF-16 units: 100 of them are 200, the most a label takes.
        let label = "\u{1F600}".repeat(100);
        let tree =
            serde_json::json!({ "version": 1, "root": { "type": "heading", "value": label } });
        assert!(check_block(&tree).problem().is_none());
        let longer = "\u{1F600}".repeat(101);
        let tree =
            serde_json::json!({ "version": 1, "root": { "type": "heading", "value": longer } });
        assert_eq!(check_block(&tree).errors[0].path, "root.value");
    }
}
