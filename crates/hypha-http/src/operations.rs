//! The admin operations, as agents call them: each admin route of the table (spores' too)
//! with its operation ID (`admin.products.update`), method, path, whether it writes, the
//! scopes it needs (`x-hypha-scopes`) and its summary. Built from the same rows as the
//! OpenAPI documents (`routes::openapi::rows`), so an operation can't exist without its
//! route or the other way round.
//!
//! Agents name operations, not routes: `Operations::get` finds one by ID, and
//! `Operation::request_path` makes the path to send it to from its path parameters.
//! `Operations::find` goes the other way, from a request's method and path.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::OnceLock,
};

use hypha_core::{OperationId, Scope};
use serde_json::{Map, Value, json};

use crate::routes::openapi::{Doc, Row};

/// The extension marking a route only a staff member's own session may call (`/me`,
/// deciding proposals, connecting a client): agents never see it as an operation.
pub const STAFF_SESSION: &str = "x-hypha-staff-session";

/// One admin operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Operation {
    pub id: OperationId,
    /// `GET`, `POST`, `PUT`, `PATCH` or `DELETE`.
    pub method: &'static str,
    /// The route's path, with its parameters (`/admin/products/{id}`).
    pub path: &'static str,
    /// Anything but a `GET`.
    pub write: bool,
    /// What the caller needs (`x-hypha-scopes`), read and write scopes alike.
    pub scopes: Vec<Scope>,
    pub summary: Option<String>,
    pub description: Option<String>,
    /// The route table's module (`products`), or the spore's tag.
    pub tag: String,
    /// Needs no credentials (logging in, a spore's public route).
    pub public: bool,
    /// Takes a staff member's own session (`x-hypha-staff-session`).
    pub session_only: bool,
}

/// One part of a path between slashes.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Segment {
    Fixed(&'static str),
    /// `{name}`: one segment.
    Param(&'static str),
    /// `{*name}`: the rest of the path.
    Rest(&'static str),
}

fn segments(path: &'static str) -> Vec<Segment> {
    path.trim_start_matches('/')
        .split('/')
        .map(
            |part| match part.strip_prefix('{').and_then(|p| p.strip_suffix('}')) {
                Some(name) => match name.strip_prefix('*') {
                    Some(rest) => Segment::Rest(rest),
                    None => Segment::Param(name),
                },
                None => Segment::Fixed(part),
            },
        )
        .collect()
}

impl Operation {
    /// Whether an agent may call it at all: it takes credentials, and not only a person's.
    pub fn for_agents(&self) -> bool {
        !self.public && !self.session_only
    }

    /// The names of its path parameters, in order.
    pub fn path_params(&self) -> Vec<&'static str> {
        segments(self.path)
            .into_iter()
            .filter_map(|s| match s {
                Segment::Param(name) | Segment::Rest(name) => Some(name),
                Segment::Fixed(_) => None,
            })
            .collect()
    }

    /// The path to send it to, with `params` (text, by name) in place, each
    /// percent-encoded but a catch-all's slashes. Every parameter is needed, and nothing
    /// else.
    pub fn request_path(&self, params: &Map<String, Value>) -> Result<String, String> {
        let names = self.path_params();
        if let Some(extra) = params.keys().find(|k| !names.contains(&k.as_str())) {
            return Err(format!("{} has no path parameter {extra:?}", self.id));
        }
        let mut path = String::new();
        for segment in segments(self.path) {
            path.push('/');
            match segment {
                Segment::Fixed(text) => path.push_str(text),
                Segment::Param(name) | Segment::Rest(name) => {
                    let value = match params.get(name) {
                        Some(Value::String(value)) if !value.is_empty() => value,
                        Some(_) => {
                            return Err(format!("path parameter {name:?} is a non-empty string"));
                        }
                        None => return Err(format!("path parameter {name:?} is missing")),
                    };
                    let rest = matches!(segment, Segment::Rest(_));
                    path.push_str(&encode(value, rest));
                }
            }
        }
        Ok(path)
    }
}

/// `value` percent-encoded for a path segment: what isn't unreserved (RFC 3986) is
/// escaped; `keep_slashes` for a catch-all, whose slashes separate segments.
pub(crate) fn encode(value: &str, keep_slashes: bool) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(char::from(byte));
            }
            b'/' if keep_slashes => out.push('/'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Every admin operation of a build: the route table's and its spores'.
#[derive(Debug, Clone, Default)]
pub struct Operations {
    by_id: BTreeMap<OperationId, Operation>,
    /// The spores' rows, for the documents `docs` reads.
    extra: Vec<Row>,
    /// What the admin document says of each operation, read once when first asked.
    docs: OnceLock<BTreeMap<OperationId, OperationDocs>>,
}

impl Operations {
    /// The route table's admin operations and `extra` rows' (a build's spores').
    pub fn build(extra: Vec<Row>) -> Self {
        let mut by_id = BTreeMap::new();
        for row in crate::routes::openapi::rows()
            .into_iter()
            .chain(extra.iter().cloned())
        {
            if row.doc != Doc::Admin {
                continue;
            }
            // The table's IDs are checked by a test; one that isn't valid has no operation.
            let Ok(id) = OperationId::new(row.operation_id.clone()) else {
                continue;
            };
            let op = (row.operation)();
            let scopes = op
                .extensions
                .as_ref()
                .and_then(|e| e.get("x-hypha-scopes"))
                .and_then(Value::as_array)
                .map(|names| {
                    names
                        .iter()
                        .filter_map(Value::as_str)
                        .filter_map(Scope::parse)
                        .collect()
                })
                .unwrap_or_default();
            let session_only = op
                .extensions
                .as_ref()
                .and_then(|e| e.get(STAFF_SESSION))
                .is_some_and(|v| v == &Value::Bool(true));
            let operation = Operation {
                id: id.clone(),
                method: row.method,
                path: row.path,
                write: row.method != "GET",
                scopes,
                summary: op.summary.clone(),
                description: op.description.clone(),
                tag: row.tag.clone(),
                public: row.public || !crate::routes::openapi::secured(&op),
                session_only,
            };
            by_id.insert(id, operation);
        }
        Self {
            by_id,
            extra,
            docs: OnceLock::new(),
        }
    }

    /// What the admin document says of `id`: its inputs and answer as self-contained JSON
    /// Schemas, and its error responses.
    pub fn docs(&self, id: &OperationId) -> Option<&OperationDocs> {
        self.docs
            .get_or_init(|| {
                let document =
                    crate::routes::openapi::document_value_with(Doc::Admin, self.extra.clone());
                self.by_id
                    .values()
                    .filter_map(|op| Some((op.id.clone(), OperationDocs::of(op, &document)?)))
                    .collect()
            })
            .get(id)
    }

    pub fn get(&self, id: &OperationId) -> Option<&Operation> {
        self.by_id.get(id)
    }

    /// Every operation, by ID.
    pub fn iter(&self) -> impl Iterator<Item = &Operation> {
        self.by_id.values()
    }
}

/// One operation as the admin document describes it, for agents: each schema
/// self-contained (its `$ref`s point into its own `$defs`), so it can be handed to a model
/// as is.
#[derive(Debug, Clone, PartialEq)]
pub struct OperationDocs {
    /// An object with a property per path parameter, all required.
    pub params: Value,
    /// An object with a property per query parameter.
    pub query: Value,
    /// The JSON body and whether it's required.
    pub body: Option<(Value, bool)>,
    /// The success response's body.
    pub output: Option<Value>,
    /// Each error response's status and description, as documented.
    pub errors: Vec<(u16, String)>,
}

const COMPONENT_SCHEMAS: &str = "#/components/schemas/";
const DEFS: &str = "#/$defs/";

impl OperationDocs {
    fn of(op: &Operation, document: &Value) -> Option<Self> {
        let path = op.path.replace("{*", "{");
        let entry = document
            .get("paths")?
            .get(&path)?
            .get(op.method.to_ascii_lowercase())?;
        let components = document
            .pointer("/components/schemas")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let parameters = entry
            .get("parameters")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let object_of = |place: &str| {
            let mut properties = Map::new();
            let mut required = Vec::new();
            for p in &parameters {
                if p.get("in").and_then(Value::as_str) != Some(place) {
                    continue;
                }
                let Some(name) = p.get("name").and_then(Value::as_str) else {
                    continue;
                };
                let mut schema = p.get("schema").cloned().unwrap_or(json!({}));
                if let (Value::Object(fields), Some(description)) =
                    (&mut schema, p.get("description").and_then(Value::as_str))
                {
                    fields
                        .entry("description")
                        .or_insert_with(|| json!(description));
                }
                properties.insert(name.to_string(), schema);
                if place == "path" || p.get("required") == Some(&Value::Bool(true)) {
                    required.push(json!(name));
                }
            }
            self_contained(
                json!({
                    "type": "object",
                    "properties": properties,
                    "required": required,
                    "additionalProperties": false,
                }),
                &components,
            )
        };
        let body = entry.get("requestBody").and_then(|body| {
            let content = body.get("content")?.as_object()?;
            let media = content
                .get("application/json")
                .or_else(|| content.values().next())?;
            let schema = self_contained(media.get("schema")?.clone(), &components);
            let required = body.get("required") == Some(&Value::Bool(true));
            Some((schema, required))
        });
        let responses = entry
            .get("responses")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let output = responses
            .iter()
            .filter(|(status, _)| status.starts_with('2'))
            .find_map(|(_, r)| r.pointer("/content/application~1json/schema"))
            .map(|schema| self_contained(schema.clone(), &components));
        let errors = responses
            .iter()
            .filter_map(|(status, r)| {
                let status: u16 = status.parse().ok()?;
                let description = r.get("description").and_then(Value::as_str)?;
                (status >= 400).then(|| (status, description.to_string()))
            })
            .collect();
        Some(Self {
            params: object_of("path"),
            query: object_of("query"),
            body,
            output,
            errors,
        })
    }
}

/// The component schema names `value` refers to.
fn refs_in(value: &Value, found: &mut BTreeSet<String>) {
    match value {
        Value::Object(fields) => {
            for (key, field) in fields {
                match (key.as_str(), field) {
                    ("$ref", Value::String(target)) => {
                        if let Some(name) = target.strip_prefix(COMPONENT_SCHEMAS) {
                            found.insert(name.to_string());
                        }
                    }
                    _ => refs_in(field, found),
                }
            }
        }
        Value::Array(items) => items.iter().for_each(|i| refs_in(i, found)),
        _ => {}
    }
}

/// `value` with its references to the document's schemas pointed at its own `$defs`.
fn point_refs_home(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            for (key, field) in fields.iter_mut() {
                match (key.as_str(), field) {
                    ("$ref", Value::String(target)) => {
                        if let Some(name) = target.strip_prefix(COMPONENT_SCHEMAS) {
                            *target = format!("{DEFS}{name}");
                        }
                    }
                    (_, field) => point_refs_home(field),
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(point_refs_home),
        _ => {}
    }
}

/// `schema` standing alone: every schema it reaches through `$ref`, transitively, copied
/// into its `$defs`.
fn self_contained(mut schema: Value, components: &Map<String, Value>) -> Value {
    let mut wanted = BTreeSet::new();
    refs_in(&schema, &mut wanted);
    let mut defs = Map::new();
    while let Some(name) = wanted.pop_first() {
        if defs.contains_key(&name) {
            continue;
        }
        let Some(mut def) = components.get(&name).cloned() else {
            continue;
        };
        let mut more = BTreeSet::new();
        refs_in(&def, &mut more);
        wanted.extend(more.into_iter().filter(|n| !defs.contains_key(n)));
        point_refs_home(&mut def);
        defs.insert(name, def);
    }
    point_refs_home(&mut schema);
    if defs.is_empty() {
        return schema;
    }
    match schema {
        Value::Object(mut fields) => {
            fields.insert("$defs".to_string(), Value::Object(defs));
            Value::Object(fields)
        }
        other => json!({"allOf": [other], "$defs": defs}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ops() -> Operations {
        Operations::build(Vec::new())
    }

    fn id(raw: &str) -> OperationId {
        OperationId::new(raw).unwrap()
    }

    #[test]
    fn every_admin_route_is_an_operation_with_its_scopes() {
        let ops = ops();
        let update = ops.get(&id("admin.products.update")).unwrap();
        assert_eq!(
            (update.method, update.path, update.write),
            ("PATCH", "/admin/products/{id}", true)
        );
        assert_eq!(update.scopes, [Scope::ProductsWrite]);
        assert_eq!(update.tag, "products");
        let list = ops.get(&id("admin.orders.list")).unwrap();
        assert!(!list.write);
        assert_eq!(list.scopes, [Scope::OrdersRead]);
        assert!(
            ops.get(&id("storefront.carts.create")).is_none(),
            "admin only"
        );
        let rows = crate::routes::openapi::rows();
        let admin = rows.iter().filter(|r| r.doc == Doc::Admin).count();
        assert_eq!(ops.iter().count(), admin, "every operation ID is valid");
    }

    #[test]
    fn an_operations_path_is_made_from_its_parameters() {
        let ops = ops();
        let op = ops.get(&id("admin.products.update_variant")).unwrap();
        assert_eq!(op.path_params(), ["id", "variant_id"]);
        let params = json!({"id": "prod_1", "variant_id": "a b/c"});
        assert_eq!(
            op.request_path(params.as_object().unwrap()),
            Ok("/admin/products/prod_1/variants/a%20b%2Fc".to_string())
        );
        for bad in [
            json!({"id": "prod_1"}),
            json!({"id": "prod_1", "variant_id": 2}),
            json!({"id": "prod_1", "variant_id": ""}),
            json!({"id": "prod_1", "variant_id": "v", "other": "x"}),
        ] {
            assert!(op.request_path(bad.as_object().unwrap()).is_err(), "{bad}");
        }
        let proxy = ops
            .get(&id("admin.app_extensions.admin_proxy_get"))
            .unwrap();
        let params = json!({"key": "app_x", "path": "a/b c"});
        assert_eq!(
            proxy.request_path(params.as_object().unwrap()),
            Ok("/admin/apps/app_x/proxy/a/b%20c".to_string())
        );
    }

    /// Every `$ref` in `schema` names one of its own `$defs`.
    fn assert_self_contained(id: &OperationId, part: &str, schema: &Value) {
        fn walk(value: &Value, out: &mut Vec<String>) {
            match value {
                Value::Object(fields) => {
                    for (k, v) in fields {
                        match (k.as_str(), v) {
                            ("$ref", Value::String(t)) => out.push(t.clone()),
                            _ => walk(v, out),
                        }
                    }
                }
                Value::Array(items) => items.iter().for_each(|i| walk(i, out)),
                _ => {}
            }
        }
        let mut targets = Vec::new();
        walk(schema, &mut targets);
        let defs = schema.get("$defs").and_then(Value::as_object);
        for target in targets {
            let name = target
                .strip_prefix(DEFS)
                .unwrap_or_else(|| panic!("{id} {part}: a reference outside: {target}"));
            assert!(
                defs.is_some_and(|d| d.contains_key(name)),
                "{id} {part}: {target} isn't in its $defs"
            );
        }
    }

    #[test]
    fn every_operations_schemas_stand_alone() {
        let ops = ops();
        let mut described = 0;
        for op in ops.iter() {
            let docs = ops
                .docs(&op.id)
                .unwrap_or_else(|| panic!("{} isn't in the admin document", op.id));
            assert_self_contained(&op.id, "params", &docs.params);
            assert_self_contained(&op.id, "query", &docs.query);
            if let Some((body, _)) = &docs.body {
                assert_self_contained(&op.id, "body", body);
            }
            if let Some(output) = &docs.output {
                assert_self_contained(&op.id, "output", output);
            }
            let params = docs.params["properties"].as_object().unwrap();
            assert_eq!(params.len(), op.path_params().len(), "{}", op.id);
            described += 1;
        }
        assert_eq!(described, ops.iter().count());
    }

    #[test]
    fn an_operation_describes_its_inputs_answer_and_errors() {
        let ops = ops();
        let docs = ops.docs(&id("admin.products.update_variant")).unwrap();
        assert_eq!(docs.params["required"], json!(["id", "variant_id"]));
        let (body, required) = docs.body.as_ref().unwrap();
        assert!(required);
        assert!(body.get("$defs").is_some() || body.get("properties").is_some());
        assert!(docs.output.is_some());
        assert!(docs.errors.iter().any(|(status, _)| *status == 404));
        let list = ops.docs(&id("admin.orders.list")).unwrap();
        assert!(list.query["properties"].get("limit").is_some());
        assert!(list.body.is_none());
    }

    #[test]
    fn staff_only_and_public_routes_are_no_agents_operations() {
        let ops = ops();
        for staff_only in [
            "admin.me.fetch",
            "admin.agents.approve",
            "admin.agents.reject",
            "admin.auth.login",
        ] {
            let op = ops.get(&id(staff_only)).unwrap();
            assert!(!op.for_agents(), "{staff_only}");
        }
        assert!(ops.get(&id("admin.orders.list")).unwrap().for_agents());
    }
}
