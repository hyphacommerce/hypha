//! The OpenAPI documents (S6-S10): one per surface, built from the route table, so every
//! routed handler is documented (a handler without `#[utoipa::path]` doesn't compile into
//! the table) and nothing unrouted is.
//!
//! Each handler's annotation gives what only it knows: its body, its success response,
//! its path parameters and its scopes (`x-hypha-scopes`). The rest is filled in here
//! from the table, the same way for every operation: the operation ID
//! (`admin.products.create`), a tag per module, the surface's security scheme, and the
//! error responses every route of that shape can give, and, on admin writes, the
//! `Hypha-Preview` header and the preview it answers (`crate::preview`).

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
                            "A staff session (`hypha_sess_…`, from `POST /admin/auth/login`), a \
                             secret API key (`hypha_sk_…`), or an app's OAuth access token \
                             (`hypha_at_…`, from `POST /oauth/token`). Callers other than staff \
                             sessions see personal data only with the `customer_data:*` scopes: \
                             without them those fields are absent and listed in `redacted`.",
                        ))
                        .build(),
                ),
            ),
            Doc::Storefront => (
                "publishable_key",
                SecurityScheme::ApiKey(ApiKey::Header(ApiKeyValue::with_description(
                    "X-Publishable-Key",
                    "A publishable API key (`hypha_pk_…`); `Authorization: Bearer` works too. A \
                     signed-in customer sends their session (`hypha_cs_…`, from `POST \
                     /storefront/auth/login`) as `Authorization: Bearer` beside the key; the \
                     routes marked `x-hypha-customer-session` need one.",
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
#[derive(Debug, Clone)]
pub struct Row {
    pub doc: Doc,
    pub method: &'static str,
    pub path: &'static str,
    /// `<surface>.<module>.<name>` (`admin.products.create`). Part of the API once
    /// clients generate from the documents: a change shows up in the committed files.
    pub operation_id: String,
    pub tag: String,
    /// Needs no credentials whatever its annotation says: a spore's public route.
    pub public: bool,
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
                tag: stringify!($module).to_string(),
                public: false,
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

pub fn rows() -> Vec<Row> {
    crate::route_table!([openapi_rows]())
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

/// The error responses every route of this shape can give, unless its annotation already
/// describes that status.
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
    if writes && row.doc == Doc::Admin {
        errors.entry("400").or_default().push(
            "A `Hypha-Preview` other than `1` or `0`, or sent with `Hypha-Preview-Token` \
             (`invalid_preview_header`).",
        );
        errors.entry("409").or_default().push(
            "With `Hypha-Preview-Token`: the token already applied its write \
             (`preview_used`), or the write would no longer record what was previewed \
             (`preview_stale`, nothing written, `details.preview` is what it would do now).",
        );
        errors.entry("422").or_default().push(
            "A `Hypha-Preview-Token` that is unknown, expired, another caller's or another \
             request's (`preview_token_invalid`).",
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
pub(crate) fn secured(op: &Operation) -> bool {
    match &op.security {
        None => true,
        Some(requirements) => requirements
            .iter()
            .any(|r| serde_json::to_value(r).is_ok_and(|v| v != serde_json::json!({}))),
    }
}

/// The document for `doc`, built from the route table.
pub fn document(doc: Doc) -> OpenApi {
    document_with(doc, Vec::new())
}

/// The document for `doc`, built from the route table and `extra` rows: a build's spores'
/// routes, tagged by spore.
pub fn document_with(doc: Doc, extra: Vec<Row>) -> OpenApi {
    let (scheme_name, scheme) = doc.security_scheme();
    let mut paths = Paths::new();
    let mut schemas: Vec<(String, RefOr<Schema>)> =
        vec![(ApiErrorBody::name().to_string(), ApiErrorBody::schema())];
    for row in rows().into_iter().chain(extra).filter(|r| r.doc == doc) {
        // The annotation repeats the table's path; they must agree (also a test).
        debug_assert_eq!((row.annotated_path)(), row.path, "{}", row.operation_id);
        debug_assert_eq!((row.annotated_methods)().len(), 1, "{}", row.operation_id);
        let mut op = (row.operation)();
        op.operation_id = Some(row.operation_id.clone());
        op.tags = Some(vec![row.tag.clone()]);
        let secured = !row.public && secured(&op);
        if secured {
            op.security = Some(vec![SecurityRequirement::new(
                scheme_name,
                Vec::<String>::new(),
            )]);
        }
        add_standard_errors(&mut op, &row, secured);
        if doc == Doc::Admin && row.method != "GET" {
            add_preview(&mut op);
        }
        (row.schemas)(&mut schemas);
        // A catch-all (`{*path}`, the app proxy) is a plain parameter in OpenAPI.
        let path = row.path.replace("{*", "{");
        paths.add_path_operation(path, vec![http_method(row.method)], op);
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
    if doc == Doc::Admin {
        add_preview_components(components);
    }
    api
}

/// The reusable `Hypha-Preview` header (`#/components/parameters/HyphaPreview`).
const PREVIEW_PARAMETER: &str = "HyphaPreview";
/// The reusable `Hypha-Preview-Token` header.
const PREVIEW_TOKEN_PARAMETER: &str = "HyphaPreviewToken";
/// The reusable answer to a preview (`#/components/responses/Preview`).
const PREVIEW_RESPONSE: &str = "Preview";

/// An admin write takes `Hypha-Preview`, and says where its preview's answer is described
/// (`x-hypha-preview`): its own `200`, where it has one, is what the write answers.
fn add_preview(op: &mut Operation) {
    use utoipa::openapi::extensions::ExtensionsBuilder;
    let parameters = op.parameters.get_or_insert_with(Vec::new);
    for name in [PREVIEW_PARAMETER, PREVIEW_TOKEN_PARAMETER] {
        parameters.push(RefOr::Ref(Ref::new(format!(
            "#/components/parameters/{name}"
        ))));
    }
    let preview = ExtensionsBuilder::new()
        .add(
            "x-hypha-preview",
            serde_json::json!({"$ref": format!("#/components/responses/{PREVIEW_RESPONSE}")}),
        )
        .build();
    match &mut op.extensions {
        Some(extensions) => extensions.merge(preview),
        None => op.extensions = Some(preview),
    }
}

/// The header, the response and their schemas, once in the admin document.
fn add_preview_components(components: &mut utoipa::openapi::Components) {
    use crate::preview::PreviewResponse;
    use utoipa::openapi::{
        Required,
        path::ParameterBuilder,
        schema::{ObjectBuilder, SchemaType, Type},
    };
    let header = ParameterBuilder::new()
        .name("Hypha-Preview")
        .parameter_in(ParameterIn::Header)
        .required(Required::False)
        .description(Some(
            "`1` previews the write: it runs for real and is rolled back, and the answer is \
             `200` with what it would have done (`#/components/responses/Preview`) instead of \
             its own. A write that fails answers its failure as without the header. `0`, or \
             no header, runs the write.",
        ))
        .schema(Some(
            ObjectBuilder::new()
                .schema_type(SchemaType::Type(Type::String))
                .enum_values(Some(["1", "0"])),
        ))
        .build();
    components
        .parameters
        .insert(PREVIEW_PARAMETER.to_string(), RefOr::T(header));
    let token = ParameterBuilder::new()
        .name("Hypha-Preview-Token")
        .parameter_in(ParameterIn::Header)
        .required(Required::False)
        .description(Some(
            "A preview's `token` (`hypha_pv_…`): sent with the same request by the same \
             caller, the write runs with the preview's IDs and commits only if it records \
             the same events, over records as they were when previewed (else `409 \
             preview_stale`, nothing written). Once \
             (`409 preview_used`); a retry sends the `Idempotency-Key` alone.",
        ))
        .schema(Some(
            ObjectBuilder::new().schema_type(SchemaType::Type(Type::String)),
        ))
        .build();
    components
        .parameters
        .insert(PREVIEW_TOKEN_PARAMETER.to_string(), RefOr::T(token));
    let response = ResponseBuilder::new()
        .description(
            "The write previewed (`Hypha-Preview: 1`): what it would have answered and \
             recorded. Nothing of it was kept.",
        )
        .content(
            "application/json",
            ContentBuilder::new()
                .schema(Some(Ref::from_schema_name(PreviewResponse::name())))
                .build(),
        )
        .build();
    components
        .responses
        .insert(PREVIEW_RESPONSE.to_string(), RefOr::T(response));
    let mut schemas = vec![(
        PreviewResponse::name().to_string(),
        PreviewResponse::schema(),
    )];
    PreviewResponse::schemas(&mut schemas);
    components.schemas.extend(schemas);
}

/// `document_with` as JSON, as the committed file holds it (open enums marked).
pub(crate) fn document_value_with(doc: Doc, extra: Vec<Row>) -> Value {
    let mut value = serde_json::to_value(document_with(doc, extra))
        .unwrap_or_else(|e| panic!("cannot serialize the OpenAPI document: {e}"));
    mark_open_enums(&mut value);
    value
}

/// The document as the committed file holds it: pretty JSON and a final newline.
pub fn document_json(doc: Doc) -> String {
    pretty(document(doc))
}

/// `document_with` as a file.
pub fn document_json_with(doc: Doc, extra: Vec<Row>) -> String {
    pretty(document_with(doc, extra))
}

/// `api` as a committed file: open enums marked, pretty JSON and a final newline.
fn pretty(api: OpenApi) -> String {
    // A document is plain data; serializing it can't fail.
    let mut value = serde_json::to_value(api)
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

/// The extension that marks an enum as open. The SDK generator turns such an enum into `"a"
/// | "b" | (string & {})`.
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

// ---------- the extensions document ----------

/// The committed file of the extensions document (`openapi/extensions.json`).
pub const EXTENSIONS_FILE: &str = "extensions.json";

/// One extension point as the document lists it: what `ExtensionPoint` says, with its
/// request and response schemas.
struct PointDoc {
    name: &'static str,
    version: u16,
    timeout: std::time::Duration,
    on_failure: hypha_extensions::OnFailure,
    summary: &'static str,
    request: (String, RefOr<Schema>),
    response: (String, RefOr<Schema>),
    schemas: Vec<(String, RefOr<Schema>)>,
}

impl PointDoc {
    fn of<P>(summary: &'static str) -> Self
    where
        P: hypha_extensions::ExtensionPoint,
        P::Request: ToSchema,
        P::Response: ToSchema,
    {
        let mut schemas = Vec::new();
        <P::Request as ToSchema>::schemas(&mut schemas);
        <P::Response as ToSchema>::schemas(&mut schemas);
        Self {
            name: P::NAME,
            version: P::VERSION,
            timeout: P::TIMEOUT,
            on_failure: P::ON_FAILURE,
            summary,
            request: (P::Request::name().to_string(), P::Request::schema()),
            response: (P::Response::name().to_string(), P::Response::schema()),
            schemas,
        }
    }
}

/// The points this build calls apps through. A new point is a line here.
fn points() -> Vec<PointDoc> {
    use hypha_extensions as x;
    vec![
        PointDoc::of::<x::ShippingRates>("Price a cart's shipping group."),
        PointDoc::of::<x::PaymentsStart>(
            "Start paying for a cart at checkout, before its order exists: what the buyer does next.",
        ),
        PointDoc::of::<x::PaymentsStatus>(
            "Say where a payment the app started is, when the shopper says they paid before the app reported it.",
        ),
        PointDoc::of::<x::PaymentsCapture>("Capture an authorized payment (staff)."),
        PointDoc::of::<x::PaymentsVoid>("Void a payment that wasn't captured (staff)."),
        PointDoc::of::<x::PaymentsRefund>("Refund some or all of a captured payment (staff)."),
        PointDoc::of::<x::SearchQuery>(
            "Answer a storefront search with product and variant IDs in order.",
        ),
        PointDoc::of::<x::RunAdminAction>("Run one of the app's admin actions, pressed by staff."),
    ]
}

/// A header Hypha sends with every extension call.
fn header(name: &str, description: &str) -> utoipa::openapi::path::Parameter {
    use utoipa::openapi::{Required, path::ParameterBuilder};
    ParameterBuilder::new()
        .name(name)
        .parameter_in(ParameterIn::Header)
        .required(Required::True)
        .description(Some(description))
        .schema(Some(String::schema()))
        .build()
}

/// One point as an OpenAPI webhook: a request Hypha sends, the answer it expects.
fn point_operation(point: &PointDoc) -> Operation {
    use utoipa::openapi::{
        Required, extensions::ExtensionsBuilder, path::OperationBuilder,
        request_body::RequestBodyBuilder,
    };
    let timeout_ms = u64::try_from(point.timeout.as_millis()).unwrap_or(u64::MAX);
    let failure = match point.on_failure {
        hypha_extensions::OnFailure::FailClosed => {
            "the flow stops with `app_unavailable` (502, `details: {app, point}`)"
        }
        hypha_extensions::OnFailure::Fallback => "Hypha falls back to its own default",
    };
    let description = format!(
        "`POST` to the URL the app registered for `{}` v{}. Answer within {timeout_ms} ms \
         (unless the app's registration sets another timeout), in at most 256 KiB, with a \
         2xx and the response below. Anything else (no answer, another status, a body that \
         isn't the response) is a failure: {failure}. Every call is in the app's call log \
         (`GET /admin/apps/{{key}}/extension-calls`).",
        point.name, point.version
    );
    let json_body = |name: &str| {
        ContentBuilder::new()
            .schema(Some(Ref::from_schema_name(name)))
            .build()
    };
    OperationBuilder::new()
        .operation_id(Some(format!("{}.v{}", point.name, point.version)))
        .tag(point.name.split('.').next().unwrap_or(point.name))
        .summary(Some(point.summary))
        .description(Some(description))
        .parameters(Some([
            header(
                crate::outbound::SIGNATURE_HEADER,
                "`t=<unix seconds>,v1=<hex HMAC-SHA256 of \"<t>.<raw body>\" under the app's \
                 secret>`. Check it, and refuse a `t` more than a few minutes old.",
            ),
            header(
                hypha_extensions::EXTENSION_HEADER,
                "The point's name, as this operation's.",
            ),
            header(
                hypha_extensions::EXTENSION_VERSION_HEADER,
                "The point's version.",
            ),
            header(
                hypha_extensions::IDEMPOTENCY_KEY_HEADER,
                "The same for every retry of one piece of work: answer a repeat as the \
                 first.",
            ),
            header(
                hypha_extensions::STORE_HEADER,
                "The store the call is for (`store_…`).",
            ),
        ]))
        .request_body(Some(
            RequestBodyBuilder::new()
                .content("application/json", json_body(&point.request.0))
                .required(Some(Required::True))
                .build(),
        ))
        .response(
            "200",
            ResponseBuilder::new()
                .description("The answer (any 2xx).")
                .content("application/json", json_body(&point.response.0))
                .build(),
        )
        .response(
            "default",
            ResponseBuilder::new()
                .description(
                    "A failure. Apps built on `@hyphacommerce/app` answer `{code, error}` \
                     (`invalid_signature`, `unknown_extension`, `unsupported_version`, \
                     `invalid_request`, `handler_failed`); the code is kept in the call log.",
                )
                .build(),
        )
        .extensions(Some(
            ExtensionsBuilder::new()
                .add(
                    "x-hypha-extension",
                    serde_json::json!({"point": point.name, "version": point.version}),
                )
                .add("x-hypha-version", point.version)
                .add("x-hypha-timeout-ms", timeout_ms)
                .add("x-hypha-on-failure", point.on_failure.as_str())
                .build(),
        ))
        .build()
}

/// The extensions document: every point as a webhook (OpenAPI 3.1's section for
/// requests the server sends), with its request and response schemas.
pub fn extensions_document() -> OpenApi {
    use utoipa::openapi::{PathItem, PathsBuilder};
    let mut schemas: Vec<(String, RefOr<Schema>)> = Vec::new();
    let mut webhooks = PathsBuilder::new();
    for point in points() {
        webhooks = webhooks.path(
            point.name,
            PathItem::new(HttpMethod::Post, point_operation(&point)),
        );
        schemas.push(point.request);
        schemas.push(point.response);
        schemas.extend(point.schemas);
    }
    let schemas: BTreeMap<String, RefOr<Schema>> = schemas.into_iter().collect();
    let mut info = Info::new("Hypha Extension Points", env!("CARGO_PKG_VERSION"));
    info.description = Some(
        "The synchronous calls Hypha makes to apps mid-flow: each is a \
         versioned contract an app implements by registering a URL for it. A reshaped \
         request or response is a new version."
            .to_string(),
    );
    let mut license = utoipa::openapi::License::new(env!("CARGO_PKG_LICENSE"));
    license.identifier = Some(env!("CARGO_PKG_LICENSE").to_string());
    info.license = Some(license);
    let mut api = OpenApiBuilder::new()
        .info(info)
        .webhooks(Some(webhooks.build()))
        .build();
    api.components
        .get_or_insert_with(Default::default)
        .schemas
        .extend(schemas);
    api
}

/// The extensions document as the committed file holds it.
pub fn extensions_document_json() -> String {
    pretty(extensions_document())
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
pub async fn admin_document<S>(State(_): State<SharedState<S>>) -> impl IntoResponse {
    serve(Doc::Admin)
}

/// `GET /storefront/openapi.json`.
pub async fn storefront_document<S>(State(_): State<SharedState<S>>) -> impl IntoResponse {
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
        // Every ISO code is accepted: a pattern, not an enum.
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
                "get /admin/agent-proposals",
                "get /admin/agents",
                "get /admin/agents/{id}/calls",
                "get /admin/api-keys",
                "get /admin/apps/{key}/deliveries",
                "get /admin/apps/{key}/extension-calls",
                "get /admin/assets",
                "get /admin/assets/{id}/usages",
                "get /admin/catalogs/{id}/products",
                "get /admin/categories/{id}/products",
                "get /admin/customers",
                "get /admin/inventory-items",
                "get /admin/notifications",
                "get /admin/orders",
                "get /admin/price-lists",
                "get /admin/price-lists/{id}/fixed-prices",
                "get /admin/products",
                "get /admin/variants"
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
                let pointer = r.strip_prefix('#').unwrap_or(&r);
                assert!(
                    json.pointer(pointer).is_some(),
                    "{doc:?}: {r} points nowhere"
                );
            }
        }
    }

    #[test]
    fn admin_writes_take_the_preview_header_and_say_what_it_answers() {
        let json = serde_json::to_value(document(Doc::Admin)).unwrap();
        for (path, item) in json["paths"].as_object().unwrap() {
            for (method, op) in item.as_object().unwrap() {
                let header = serde_json::json!({"$ref": "#/components/parameters/HyphaPreview"});
                let takes = op["parameters"]
                    .as_array()
                    .is_some_and(|p| p.contains(&header));
                let writes = method != "get";
                assert_eq!(takes, writes, "{method} {path}");
                assert_eq!(
                    op.get("x-hypha-preview").is_some(),
                    writes,
                    "{method} {path}"
                );
            }
        }
        let storefront = serde_json::to_value(document(Doc::Storefront)).unwrap();
        assert!(storefront["components"]["parameters"].is_null());
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
        let documents = Doc::ALL
            .map(|doc| (doc.file_name(), document_json(doc)))
            .into_iter()
            .chain([(EXTENSIONS_FILE, extensions_document_json())]);
        for (file, json) in documents {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../openapi")
                .join(file);
            let committed = std::fs::read_to_string(&path).unwrap_or_default();
            assert!(
                committed == json,
                "{} is out of date: run `cargo run -p hypha-host-axum -- openapi`",
                path.display()
            );
        }
    }

    #[test]
    fn every_extension_point_is_a_webhook_with_its_schemas() {
        let json = serde_json::to_value(extensions_document()).unwrap();
        let webhooks = json["webhooks"].as_object().unwrap();
        assert_eq!(
            webhooks.keys().map(String::as_str).collect::<Vec<_>>(),
            [
                "admin.action",
                "payments.capture",
                "payments.refund",
                "payments.start",
                "payments.status",
                "payments.void",
                "search.query",
                "shipping.rates"
            ]
        );
        let rates = &webhooks["shipping.rates"]["post"];
        assert_eq!(rates["operationId"], "shipping.rates.v1");
        assert_eq!(rates["x-hypha-on-failure"], "fallback");
        assert_eq!(
            rates["x-hypha-extension"],
            serde_json::json!({"point": "shipping.rates", "version": 1})
        );
        assert_eq!(
            webhooks["payments.start"]["post"]["x-hypha-timeout-ms"],
            15000
        );
        let mut found = Vec::new();
        refs(&json, &mut found);
        assert!(!found.is_empty());
        for r in found {
            let name = r.strip_prefix("#/components/schemas/").unwrap_or(&r);
            assert!(
                json["components"]["schemas"].get(name).is_some(),
                "{r} points nowhere"
            );
        }
        // The address is the APIs' own schema, by the same name.
        assert!(json["components"]["schemas"]["PostalAddressResponse"].is_object());
    }
}
