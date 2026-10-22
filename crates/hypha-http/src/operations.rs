//! The admin operations, as agents call them: each admin route of the table (spores' too)
//! with its operation ID (`admin.products.update`), method, path, whether it writes, the
//! scopes it needs (`x-hypha-scopes`) and its summary. Built from the same rows as the
//! OpenAPI documents (`routes::openapi::rows`), so an operation can't exist without its
//! route or the other way round.
//!
//! Agents name operations, not routes: `Operations::get` finds one by ID, and
//! `Operation::request_path` makes the path to send it to from its path parameters.
//! `Operations::find` goes the other way, from a request's method and path.

use std::collections::BTreeMap;

use hypha_core::{OperationId, Scope};
use serde_json::{Map, Value};

use crate::routes::openapi::{Doc, Row};

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

    /// The path parameters `path` gives this operation's route, if it's one of its paths.
    fn matches(&self, path: &str) -> Option<(Vec<bool>, Map<String, Value>)> {
        let mut parts = path.trim_start_matches('/').split('/');
        let mut fixed = Vec::new();
        let mut params = Map::new();
        for segment in segments(self.path) {
            match segment {
                Segment::Fixed(text) => {
                    if parts.next()? != text {
                        return None;
                    }
                    fixed.push(true);
                }
                Segment::Param(name) => {
                    let part = parts.next().filter(|p| !p.is_empty())?;
                    params.insert(name.to_string(), Value::String(decode(part)?));
                    fixed.push(false);
                }
                Segment::Rest(name) => {
                    let rest: Vec<&str> = parts.by_ref().collect();
                    params.insert(name.to_string(), Value::String(decode(&rest.join("/"))?));
                    fixed.push(false);
                }
            }
        }
        parts.next().is_none().then_some((fixed, params))
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

/// A path segment percent-decoded; `None` if that isn't UTF-8 or an escape is broken.
fn decode(part: &str) -> Option<String> {
    let bytes = part.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = part.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Every admin operation of a build: the route table's and its spores'.
#[derive(Debug, Clone, Default)]
pub struct Operations {
    by_id: BTreeMap<OperationId, Operation>,
}

impl Operations {
    /// The route table's admin operations and `extra` rows' (a build's spores').
    pub fn build(extra: Vec<Row>) -> Self {
        let mut by_id = BTreeMap::new();
        for row in crate::routes::openapi::rows().into_iter().chain(extra) {
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
            let operation = Operation {
                id: id.clone(),
                method: row.method,
                path: row.path,
                write: row.method != "GET",
                scopes,
                summary: op.summary.clone(),
                description: op.description.clone(),
                tag: row.tag.clone(),
            };
            by_id.insert(id, operation);
        }
        Self { by_id }
    }

    pub fn get(&self, id: &OperationId) -> Option<&Operation> {
        self.by_id.get(id)
    }

    /// Every operation, by ID.
    pub fn iter(&self) -> impl Iterator<Item = &Operation> {
        self.by_id.values()
    }

    /// The operation a request to `method path` reaches, with the path parameters it
    /// gives it. Where two routes match (`…/suggestions` and `…/{id}`), the one with a
    /// fixed segment earlier wins, as the router decides.
    pub fn find(&self, method: &str, path: &str) -> Option<(&Operation, Map<String, Value>)> {
        self.by_id
            .values()
            .filter(|op| op.method == method)
            .filter_map(|op| op.matches(path).map(|(fixed, params)| (fixed, op, params)))
            .max_by(|a, b| a.0.cmp(&b.0))
            .map(|(_, op, params)| (op, params))
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
    fn a_request_finds_its_operation_and_parameters() {
        let ops = ops();
        let (op, params) = ops
            .find("PATCH", "/admin/products/prod_1/variants/var_2")
            .unwrap();
        assert_eq!(op.id, id("admin.products.update_variant"));
        assert_eq!(
            params,
            json!({"id": "prod_1", "variant_id": "var_2"})
                .as_object()
                .unwrap()
                .clone()
        );
        let (op, _) = ops
            .find("GET", "/admin/custom-field-definitions/suggestions")
            .unwrap();
        assert_eq!(
            op.id,
            id("admin.custom_fields.suggestions"),
            "the fixed segment wins"
        );
        let (op, params) = ops.find("POST", "/admin/apps/app_x/proxy/a/b%20c").unwrap();
        assert_eq!(op.id, id("admin.app_extensions.admin_proxy_post"));
        assert_eq!(params["path"], "a/b c");
        assert!(ops.find("GET", "/admin/products/prod_1/nope").is_none());
        assert!(ops.find("PUT", "/admin/products").is_none());
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
}
