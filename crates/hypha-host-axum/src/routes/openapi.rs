//! The OpenAPI documents (design note 0010, S6-S10): one per surface, built from the
//! route table, so every routed handler is documented (a handler without
//! `#[utoipa::path]` doesn't compile into the table) and nothing unrouted is.
//!
//! Each handler's annotation gives what only it knows: its body, its success response,
//! its path parameters and its scopes (`x-hypha-scopes`). The rest is filled in here
//! from the table, the same way for every operation: the operation ID
//! (`admin.products.create`), a tag per module, the surface's security scheme, and the
//! error responses every route of that shape can give.

use std::{collections::BTreeMap, sync::OnceLock};

use axum::{extract::State, http::header::CONTENT_TYPE, response::IntoResponse};
use hypha_core::PageLimit;
use serde_json::Value;
use utoipa::{
    PartialSchema, ToSchema,
    openapi::{
        ContentBuilder, Info, OpenApi, OpenApiBuilder, Paths, Ref, RefOr, ResponseBuilder, Schema,
        path::{HttpMethod, Operation, ParameterIn},
        security::{
            ApiKey, ApiKeyValue, HttpAuthScheme, HttpBuilder, SecurityRequirement, SecurityScheme,
        },
    },
};

use super::page;
use crate::{error::ApiErrorBody, state::SharedState};

/// Which document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Doc {
    Admin,
    Storefront,
}

impl Doc {
    pub const ALL: [Doc; 2] = [Doc::Admin, Doc::Storefront];

    /// The committed file's name (`openapi/<name>.json`).
    pub fn file_name(self) -> &'static str {
        match self {
            Doc::Admin => "admin.json",
            Doc::Storefront => "storefront.json",
        }
    }

    fn security_scheme(self) -> (&'static str, SecurityScheme) {
        match self {
            Doc::Admin => (
                "bearer",
                SecurityScheme::Http(
                    HttpBuilder::new()
                        .scheme(HttpAuthScheme::Bearer)
                        .description(Some(
                            "A staff session (`hypha_sess_…`, from `POST /admin/auth/login`) or a \
                             secret API key (`hypha_sk_…`).",
                        ))
                        .build(),
                ),
            ),
            Doc::Storefront => (
                "publishable_key",
                SecurityScheme::ApiKey(ApiKey::Header(ApiKeyValue::with_description(
                    "X-Publishable-Key",
                    "A publishable API key (`hypha_pk_…`); `Authorization: Bearer` works too.",
                ))),
            ),
        }
    }

    fn info(self) -> Info {
        let (title, description) = match self {
            Doc::Admin => (
                "Hypha Admin API",
                "Running the store: catalog, markets and pricing, tax, inventory, customers, \
                 orders, staff and API keys. Staff sessions and secret keys only.",
            ),
            Doc::Storefront => (
                "Hypha Storefront API",
                "What a shop front needs: the store, active products, carts and checkout. \
                 Publishable keys only.",
            ),
        };
        let mut info = Info::new(title, env!("CARGO_PKG_VERSION"));
        info.description = Some(description.to_string());
        let mut license = utoipa::openapi::License::new(env!("CARGO_PKG_LICENSE"));
        license.identifier = Some(env!("CARGO_PKG_LICENSE").to_string());
        info.license = Some(license);
        info
    }
}

/// One route of a document, as the route table and the handler's annotation give it.
pub(crate) struct Row {
    pub doc: Doc,
    pub method: &'static str,
    pub path: &'static str,
    /// `<surface>.<module>.<name>` (`admin.products.create`). Part of the API once
    /// clients generate from the documents: a change shows up in the committed files.
    pub operation_id: String,
    pub tag: &'static str,
    /// What the handler's `#[utoipa::path]` says (checked against `method` and `path`).
    pub annotated_path: fn() -> String,
    pub annotated_methods: fn() -> Vec<HttpMethod>,
    pub operation: fn() -> Operation,
    pub schemas: fn(&mut Vec<(String, RefOr<Schema>)>),
}

/// `route_table!` callback: a `Row` per documented route (hidden ones are skipped).
macro_rules! openapi_rows {
    ($([$surface:ident, $method:ident, $path:literal, $module:ident::$func:ident]),* $(,)?) => {{
        let mut rows: Vec<Row> = Vec::new();
        $( openapi_rows!(@row rows, $surface, $method, $path, $module, $func); )*
        rows
    }};
    (@row $rows:ident, hidden, $($rest:tt)*) => {};
    (@row $rows:ident, $surface:ident, $method:ident, $path:literal, $module:ident, $func:ident) => {
        pastey::paste! {
            $rows.push(Row {
                doc: openapi_rows!(@doc $surface),
                method: stringify!($method),
                path: $path,
                operation_id: operation_id(
                    stringify!($surface),
                    stringify!($module),
                    stringify!($func),
                ),
                tag: stringify!($module),
                annotated_path: <super::$module::[<__path_ $func>] as utoipa::Path>::path,
                annotated_methods: <super::$module::[<__path_ $func>] as utoipa::Path>::methods,
                operation: <super::$module::[<__path_ $func>] as utoipa::Path>::operation,
                schemas: <super::$module::[<__path_ $func>] as utoipa::__dev::SchemaReferences>::schemas,
            });
        }
    };
    (@doc admin) => { Doc::Admin };
    (@doc storefront) => { Doc::Storefront };
}

/// The operation ID for a handler: its surface, module and name, without the
/// `_handler` and `_storefront` suffixes that only keep Rust names apart
/// (`storefront.products.fetch`, not `storefront.products.fetch_storefront`).
fn operation_id(surface: &str, module: &str, func: &str) -> String {
    let name = func.strip_suffix("_handler").unwrap_or(func);
    let name = name.strip_suffix("_storefront").unwrap_or(name);
    format!("{surface}.{module}.{name}")
}

pub(crate) fn rows() -> Vec<Row> {
    route_table!(openapi_rows!())
}

fn http_method(method: &str) -> HttpMethod {
    match method {
        "GET" => HttpMethod::Get,
        "POST" => HttpMethod::Post,
        "PUT" => HttpMethod::Put,
        "PATCH" => HttpMethod::Patch,
        "DELETE" => HttpMethod::Delete,
        // The route table only uses the five above.
        other => panic!("route table method {other} has no OpenAPI mapping"),
    }
}

/// An error response with the shared error body.
fn error_response(description: &str) -> RefOr<utoipa::openapi::Response> {
    ResponseBuilder::new()
        .description(description)
        .content(
            "application/json",
            ContentBuilder::new()
                .schema(Some(Ref::from_schema_name(ApiErrorBody::name())))
                .build(),
        )
        .build()
        .into()
}

/// The error responses every route of this shape can give (design note 0010, S9),
/// unless its annotation already describes that status.
fn add_standard_errors(op: &mut Operation, row: &Row, secured: bool) {
    let has_body = op.request_body.is_some();
    let has_path_params = row.path.contains('{');
    let query: Vec<&str> = op
        .parameters
        .iter()
        .flatten()
        .filter_map(|p| match p {
            RefOr::T(p) if p.parameter_in == ParameterIn::Query => Some(p.name.as_str()),
            _ => None,
        })
        .collect();
    let writes = row.method != "GET";
    let invalid_limit = format!(
        "A `limit` outside 1 to {} (`invalid_limit`).",
        PageLimit::MAX
    );
    let mut errors: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    if has_path_params {
        errors
            .entry("400")
            .or_default()
            .push("A malformed ID in the path (`invalid_path`).");
        errors
            .entry("404")
            .or_default()
            .push("The resource in the path doesn't exist (`<resource>_not_found`).");
    }
    if !query.is_empty() {
        errors
            .entry("400")
            .or_default()
            .push("An unknown or malformed query parameter (`invalid_query`).");
    }
    if is_list(op) {
        errors
            .entry("400")
            .or_default()
            .push("A `cursor` this list didn't return (`invalid_cursor`).");
        errors.entry("422").or_default().push(&invalid_limit);
    }
    if has_body {
        errors
            .entry("400")
            .or_default()
            .push("A body that isn't JSON (`malformed_json`).");
        errors
            .entry("413")
            .or_default()
            .push("A body over the size limit, 2 MiB unless configured (`body_too_large`).");
        errors
            .entry("415")
            .or_default()
            .push("No `Content-Type: application/json` (`missing_content_type`).");
        errors.entry("422").or_default().push(
            "Invalid input: unknown fields (`invalid_body`), values that break a rule \
             (`field_required`, `field_too_long`, …), or IDs in the body that don't exist.",
        );
    }
    if secured {
        errors.entry("401").or_default().push(
            "No credentials (`unauthenticated`), or unknown, expired or revoked ones \
             (`invalid_token`).",
        );
        errors.entry("403").or_default().push(
            "A missing scope (`missing_scope`, see `x-hypha-scopes`), or credentials for the \
             other API (`admin_credentials_required`, `publishable_key_required`).",
        );
    }
    if writes {
        errors.entry("409").or_default().push(
            "Conflicts with existing data or the resource's state, or a concurrent change \
             (`concurrent_modification`, safe to retry).",
        );
    }
    errors
        .entry("500")
        .or_default()
        .push("A server failure (`internal_error`).");
    for (status, lines) in errors {
        op.responses
            .responses
            .entry(status.to_string())
            .or_insert_with(|| error_response(&lines.join(" ")));
    }
}

/// Whether the operation is a list: it takes `PageParams`' parameters, exactly (a route
/// with an unrelated `limit` is not one).
fn is_list(op: &Operation) -> bool {
    let params = op.parameters.as_deref().unwrap_or_default();
    page::page_params().into_iter().all(|wanted| {
        params
            .iter()
            .any(|p| matches!(p, RefOr::T(p) if *p == wanted))
    })
}

/// Whether the operation needs credentials: annotated `security(())` means none.
fn secured(op: &Operation) -> bool {
    match &op.security {
        None => true,
        Some(requirements) => requirements
            .iter()
            .any(|r| serde_json::to_value(r).is_ok_and(|v| v != serde_json::json!({}))),
    }
}

/// The document for `doc`, built from the route table.
pub fn document(doc: Doc) -> OpenApi {
    let (scheme_name, scheme) = doc.security_scheme();
    let mut paths = Paths::new();
    let mut schemas: Vec<(String, RefOr<Schema>)> =
        vec![(ApiErrorBody::name().to_string(), ApiErrorBody::schema())];
    for row in rows().into_iter().filter(|r| r.doc == doc) {
        // The annotation repeats the table's path; they must agree (also a test).
        debug_assert_eq!((row.annotated_path)(), row.path, "{}", row.operation_id);
        debug_assert_eq!((row.annotated_methods)().len(), 1, "{}", row.operation_id);
        let mut op = (row.operation)();
        op.operation_id = Some(row.operation_id.clone());
        op.tags = Some(vec![row.tag.to_string()]);
        let secured = secured(&op);
        if secured {
            op.security = Some(vec![SecurityRequirement::new(
                scheme_name,
                Vec::<String>::new(),
            )]);
        }
        add_standard_errors(&mut op, &row, secured);
        (row.schemas)(&mut schemas);
        paths.add_path_operation(row.path, vec![http_method(row.method)], op);
    }
    // One schema per name; `tests::schema_names_are_unique` checks two types never
    // share one.
    let schemas: BTreeMap<String, RefOr<Schema>> = schemas.into_iter().collect();
    let mut api = OpenApiBuilder::new()
        .info(doc.info())
        // Every deployment has its own host, so paths are relative to wherever it runs.
        .servers(Some([utoipa::openapi::Server::new("/")]))
        .paths(paths)
        .build();
    let components = api.components.get_or_insert_with(Default::default);
    components.schemas.extend(schemas);
    components.add_security_scheme(scheme_name, scheme);
    api
}

/// The document as the committed file holds it: pretty JSON and a final newline.
pub fn document_json(doc: Doc) -> String {
    // A document is plain data; serializing it can't fail.
    let mut value = serde_json::to_value(document(doc))
        .unwrap_or_else(|e| panic!("cannot serialize the OpenAPI document: {e}"));
    mark_open_enums(&mut value);
    let mut json = serde_json::to_string_pretty(&value)
        .unwrap_or_else(|e| panic!("cannot serialize the OpenAPI document: {e}"));
    json.push('\n');
    json
}

/// Enum schemas that only requests use: the server checks them, and a client never
/// meets a value it doesn't know, so they stay closed.
const CLOSED_ENUMS: &[&str] = &["KindRequest", "StatusRequest"];

/// The extension that marks an enum as open (design note 0016, section 9). The SDK
/// generator turns such an enum into `"a" | "b" | (string & {})`.
pub(crate) const OPEN_ENUM: &str = "x-hypha-open-enum";

/// Marks every enum a response can carry as open: statuses, currencies, kinds and
/// scopes gain values as Hypha grows (fulfilment adds states), and a client that
/// switched exhaustively over today's values must not break on tomorrow's.
///
/// Named enum schemas are open. An inline enum is open only with several values: one
/// value is a tagged union's tag (`"type": "builtin"`), which must stay a literal so
/// clients can tell the variants apart.
fn mark_open_enums(document: &mut Value) {
    fn walk(node: &mut Value, named: bool, closed: bool) {
        match node {
            Value::Object(map) => {
                let open = match map.get("enum") {
                    Some(Value::Array(values)) => !closed && (named || values.len() > 1),
                    _ => false,
                };
                if let Some(Value::Array(values)) = map.get("enum").filter(|_| open) {
                    let known: Vec<String> = values
                        .iter()
                        .filter_map(Value::as_str)
                        .map(|v| format!("`{v}`"))
                        .collect();
                    let note = format!(
                        "Known values: {}. More may be added: treat an unknown one as you \
                         would an unexpected state, not as an error.",
                        known.join(", ")
                    );
                    map.insert(OPEN_ENUM.to_string(), Value::Bool(true));
                    let description = match map.get("description").and_then(Value::as_str) {
                        Some(text) => format!("{text}\n\n{note}"),
                        None => note,
                    };
                    map.insert("description".to_string(), Value::String(description));
                }
                for child in map.values_mut() {
                    walk(child, false, false);
                }
            }
            Value::Array(items) => items.iter_mut().for_each(|item| walk(item, false, false)),
            _ => {}
        }
    }
    let Some(schemas) = document
        .pointer_mut("/components/schemas")
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    for (name, schema) in schemas.iter_mut() {
        walk(schema, true, CLOSED_ENUMS.contains(&name.as_str()));
    }
    // Paths too: query parameters with inline enums (a list's `status` filter).
    if let Some(paths) = document.get_mut("paths") {
        walk(paths, false, false);
    }
}

/// The document's JSON, built once per process: it only changes with the code.
fn cached(doc: Doc) -> &'static str {
    static ADMIN: OnceLock<String> = OnceLock::new();
    static STOREFRONT: OnceLock<String> = OnceLock::new();
    let cell = match doc {
        Doc::Admin => &ADMIN,
        Doc::Storefront => &STOREFRONT,
    };
    cell.get_or_init(|| document_json(doc))
}

fn serve(doc: Doc) -> impl IntoResponse {
    ([(CONTENT_TYPE, "application/json")], cached(doc))
}

/// `GET /admin/openapi.json`.
pub(super) async fn admin_document<S>(State(_): State<SharedState<S>>) -> impl IntoResponse {
    serve(Doc::Admin)
}

/// `GET /storefront/openapi.json`.
pub(super) async fn storefront_document<S>(State(_): State<SharedState<S>>) -> impl IntoResponse {
    serve(Doc::Storefront)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_enums_are_open_and_request_only_ones_closed() {
        let json: Value = serde_json::from_str(&document_json(Doc::Admin)).unwrap();
        let schema = |name: &str| json["components"]["schemas"][name].clone();
        assert_eq!(schema("FulfillmentStatusDto")[OPEN_ENUM], Value::Bool(true));
        // Every ISO code is accepted (design note 0021, SU1): a pattern, not an enum.
        assert_eq!(schema("Currency").get("enum"), None);
        assert_eq!(schema("KindRequest").get(OPEN_ENUM), None);
    }

    #[test]
    fn every_annotation_names_its_route_table_row() {
        for row in rows() {
            assert_eq!((row.annotated_path)(), row.path, "{}", row.operation_id);
            let methods = serde_json::to_value((row.annotated_methods)()).unwrap();
            assert_eq!(
                methods,
                serde_json::json!([row.method.to_lowercase()]),
                "{}",
                row.operation_id
            );
        }
    }

    #[test]
    fn operation_ids_are_unique_and_plain() {
        let mut seen = std::collections::HashSet::new();
        for row in rows() {
            assert!(
                seen.insert(row.operation_id.clone()),
                "{} twice",
                row.operation_id
            );
            assert!(
                !row.operation_id.ends_with("_handler")
                    && !row.operation_id.ends_with("_storefront"),
                "{}",
                row.operation_id
            );
        }
        assert_eq!(
            operation_id("storefront", "products", "fetch_storefront"),
            "storefront.products.fetch"
        );
    }

    #[test]
    fn only_lists_get_the_paging_errors() {
        let json: Value = serde_json::from_str(cached(Doc::Admin)).unwrap();
        let mut lists = Vec::new();
        for (path, item) in json["paths"].as_object().unwrap() {
            for (method, op) in item.as_object().unwrap() {
                let errors = op["responses"].to_string();
                if errors.contains("invalid_cursor") {
                    assert!(errors.contains("invalid_limit"), "{method} {path}");
                    lists.push(format!("{method} {path}"));
                }
            }
        }
        assert_eq!(
            lists,
            [
                "get /admin/api-keys",
                "get /admin/apps/{key}/deliveries",
                "get /admin/assets",
                "get /admin/assets/{id}/usages",
                "get /admin/catalogs/{id}/products",
                "get /admin/categories/{id}/products",
                "get /admin/orders",
                "get /admin/price-lists/{id}/fixed-prices",
                "get /admin/products"
            ]
        );
    }

    #[test]
    fn the_document_is_served_as_json_text() {
        let json: Value = serde_json::from_str(cached(Doc::Storefront)).unwrap();
        assert_eq!(json["info"]["title"], "Hypha Storefront API");
    }

    #[test]
    fn schema_names_are_unique() {
        // Two types with one name would silently replace each other in the document.
        let mut by_name: BTreeMap<String, Value> = BTreeMap::new();
        for row in rows() {
            let mut schemas = Vec::new();
            (row.schemas)(&mut schemas);
            for (name, schema) in schemas {
                let schema = serde_json::to_value(schema).unwrap();
                if let Some(seen) = by_name.insert(name.clone(), schema.clone()) {
                    assert_eq!(seen, schema, "two different schemas named {name}");
                }
            }
        }
    }

    /// Every `$ref` in `value`.
    fn refs(value: &Value, out: &mut Vec<String>) {
        match value {
            Value::Object(map) => {
                for (key, v) in map {
                    match (key.as_str(), v) {
                        ("$ref", Value::String(r)) => out.push(r.clone()),
                        _ => refs(v, out),
                    }
                }
            }
            Value::Array(items) => items.iter().for_each(|v| refs(v, out)),
            _ => {}
        }
    }

    #[test]
    fn every_reference_resolves() {
        for doc in Doc::ALL {
            let json = serde_json::to_value(document(doc)).unwrap();
            let mut found = Vec::new();
            refs(&json, &mut found);
            assert!(!found.is_empty());
            for r in found {
                let name = r.strip_prefix("#/components/schemas/").unwrap_or(&r);
                assert!(
                    json["components"]["schemas"].get(name).is_some(),
                    "{doc:?}: {r} points nowhere"
                );
            }
        }
    }

    #[test]
    fn every_operation_has_an_id_a_scope_note_and_responses() {
        for doc in Doc::ALL {
            let json = serde_json::to_value(document(doc)).unwrap();
            for (path, item) in json["paths"].as_object().unwrap() {
                for (method, op) in item.as_object().unwrap() {
                    let at = format!("{doc:?} {method} {path}");
                    assert!(op["operationId"].is_string(), "{at}");
                    assert!(op["responses"]["500"].is_object(), "{at}");
                    let public = op["security"] == serde_json::json!([{}]);
                    let staff_only = op["x-hypha-staff-session"] == Value::Bool(true);
                    assert!(
                        public || staff_only || op["x-hypha-scopes"].is_array(),
                        "{at} says nothing about the scope it needs"
                    );
                }
            }
        }
    }

    #[test]
    fn the_committed_documents_are_up_to_date() {
        for doc in Doc::ALL {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../openapi")
                .join(doc.file_name());
            let committed = std::fs::read_to_string(&path).unwrap_or_default();
            assert!(
                committed == document_json(doc),
                "{} is out of date: run `cargo run -p hypha-host-axum -- openapi`",
                path.display()
            );
        }
    }
}
