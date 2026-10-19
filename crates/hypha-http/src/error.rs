//! Maps every crate's errors to HTTP statuses. An unknown ID in a body is 422 and one
//! in the URL is 404 (`not_found_if`).
//!
//! Exception: these routes store IDs from *another* domain without checking them, because
//! a leaf crate can't see other domains: variants in fixed prices
//! (`PUT /admin/price-lists/{id}/fixed-prices`), products in catalogs (`POST
//! /admin/catalogs/{id}/products`), products in tax assignments (`PUT
//! /admin/tax-assignments`), markets
//! of a location, the variant of a new inventory item, and the tax class of a shipping
//! option (`POST`/`PATCH /admin/shipping-zones/{id}/options…`). An unknown one is stored and
//! simply never matches. Checking them belongs in a composing layer (like `hypha-cart`),
//! not in these crates. Likewise a payment method's markets and channels
//! (`POST`/`PATCH /admin/payment-methods…`).

use axum::{
    Json,
    extract::rejection::{JsonRejection, PathRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use hypha_core::{
    CountryError, CurrencyError, ErrorCode, OrderId, PageError, RepoError, VariantId,
};
use serde_json::{Value, json};

/// Every error body, for the OpenAPI documents (design note 0010, S9). `ApiError`
/// serializes as this shape.
#[derive(serde::Serialize, utoipa::ToSchema)]
#[schema(as = Error)]
pub struct ApiErrorBody {
    /// Stable snake_case code: what clients match on.
    pub code: String,
    /// For humans; may change.
    pub error: String,
    /// Data a client acts on, for the codes that have any (see each operation).
    pub details: Option<serde_json::Value>,
}

/// Every error the API returns, as
/// `{"code": "<stable_code>", "error": "<message>", "details": {…} | null}`.
///
/// `code` comes from the domain's `ErrorCode` impls (shared with future MCP/CLI hosts)
/// and is what clients should match on; `error` is for humans and may change.
/// `details` carries what a client needs to act without parsing `error`, for the codes
/// that have any (field names are as stable as codes):
/// - `out_of_stock`: `{"lines": [{"variant", "requested", "available"}]}`
/// - `cart_unavailable`: `{"lines": ["var_…"]}`, the lines `GET /storefront/carts/{id}` explains
/// - `total_changed`: `{"expected": Money, "actual": Money}`
/// - `cart_completed`: `{"order": "ord_…"}`
/// - `mfa_locked`: `{"locked_until": "<RFC 3339>"}`
/// - `rate_limited`: `{"retry_after_secs": n}`, also sent as `Retry-After`
/// - `option_value_in_use`: `{"variants": ["var_…"]}`, to remove with the values
/// - `duplicate_option_combination` (removing an option): `{"variants": ["var_…", "var_…"]}`
/// - `sku_taken` (another product): `{"sku", "product": "prod_…"}`
/// - `shipping_option_not_offered`: `{"reason"}`: `no_price_in_currency`, `weight_unknown`,
///   `provider_failed`, `service_not_quoted` or `backup_not_needed`
/// - `shipping_option_unavailable` (checkout): `{"reason"}`: `shipping_option_unavailable`,
///   `no_price_in_currency`, `weight_unknown` or a money error code; pick again
/// - `over_fulfilled`: `{"line": "ordl_…", "requested": n, "unfulfilled": n}`, what the
///   line has left to ship
/// - `order_has_fulfillments`: `{"fulfillments": ["ful_…"]}`, the standing ones to cancel
///   first
/// - `asset_in_use`: `{"usages": [{"owner": {"kind", "id"}, "role"}], "has_more"}`, the first
///   25; `GET /admin/assets/{id}/usages` pages through all of them
/// - `template_invalid`: `{"part", "line", "message"}`: `part` is `subject`, `html`,
///   `text` or `layout`, `line` counts from 1 (`null` when unknown)
/// - `app_unavailable`: `{"app", "point"}`, the app and the extension point that gave no
///   usable answer (why is in the app's call log)
/// - `payment_refused`: `{"reason"}`, the payment provider's words
/// - `order_not_payable`: `{"financial_status"}`
///
/// Status rules: 401 no or bad credentials (with `WWW-Authenticate: Bearer`); 429 over
/// a rate limit or locked second factor; 403
/// missing scope, or granting a scope the caller lacks; 404 when the resource in the URL
/// doesn't exist;
/// 409 conflicts with existing data, the current state of the resource in the URL, or a
/// concurrent change (`concurrent_modification`); 422 invalid input, including IDs in the
/// body that don't exist; 500 storage failures (message hidden, logged instead); 502 an
/// email that could not be handed over (the write before it stands): the staff flows'
/// `notification_failed`, and for test sends the provider's own words,
/// `email_unavailable` (unreachable, try again) or `email_refused` (refused for good).
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
    details: Option<serde_json::Value>,
    /// Seconds for a `Retry-After` header (429 only).
    retry_after_secs: Option<u64>,
}

impl ApiError {
    fn new(status: StatusCode, code: &'static str, message: impl ToString) -> Self {
        Self {
            status,
            code,
            message: message.to_string(),
            details: None,
            retry_after_secs: None,
        }
    }

    /// Over a rate limit (design note 0009, R6). The wait is rounded up to whole
    /// seconds, at least one, so a client that waits that long gets through.
    pub fn rate_limited(retry_after: std::time::Duration) -> Self {
        let secs = retry_after.as_secs() + u64::from(retry_after.subsec_nanos() > 0);
        let secs = secs.max(1);
        let mut e = Self::new(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            format!("too many requests; try again in {secs} s"),
        )
        .with_details(Some(json!({ "retry_after_secs": secs })));
        e.retry_after_secs = Some(secs);
        e
    }

    fn with_details(mut self, details: Option<serde_json::Value>) -> Self {
        self.details = details;
        self
    }

    pub fn body_too_large(limit: usize) -> Self {
        Self::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "body_too_large",
            format!("the request body is larger than {limit} bytes"),
        )
    }

    /// `/oauth/authorize` without `server.admin_url`: there is no consent page to send
    /// the browser to (design note 0029, §4.1).
    pub fn admin_url_missing() -> Self {
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "admin_url_not_configured",
            "set server.admin_url (HYPHA_ADMIN_URL): apps are approved on the admin's consent page",
        )
    }

    /// No credentials at all (design note 0008, section 3).
    pub fn unauthenticated() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            "unauthenticated",
            "send a session or API key as `Authorization: Bearer <token>`",
        )
    }

    /// An admin route called with a publishable key (design note 0010, S3).
    pub fn admin_credentials_required() -> Self {
        Self::new(
            StatusCode::FORBIDDEN,
            "admin_credentials_required",
            "admin routes take a staff session or a secret key, not a publishable key",
        )
    }

    /// A storefront route called with something other than a publishable key.
    pub fn publishable_key_required() -> Self {
        Self::new(
            StatusCode::FORBIDDEN,
            "publishable_key_required",
            "storefront routes take a publishable key (`X-Publishable-Key`)",
        )
    }

    /// A staff-only route called with an API key.
    pub fn session_required() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            "session_required",
            "this route needs a staff session, not an API key",
        )
    }

    /// For routes where a wrong code or password comes from someone already logged in:
    /// a 401 would tell clients their session died.
    pub fn unprocessable_if(mut self, code: &str) -> Self {
        if self.code == code {
            self.status = StatusCode::UNPROCESSABLE_ENTITY;
        }
        self
    }

    /// The body this error answers with, for an answer that carries an error inside a
    /// success (checkout's `payment_error`).
    pub(crate) fn body(&self) -> ApiErrorBody {
        ApiErrorBody {
            code: self.code.to_string(),
            error: self.message.clone(),
            details: self.details.clone(),
        }
    }

    /// The code, for tests outside this module.
    #[cfg(test)]
    pub(crate) fn code(&self) -> &'static str {
        self.code
    }

    /// An app proxy path with a segment that would climb out of the app's proxy URL
    /// (`.`, `..`, in any encoding) or a backslash (design note 0029, §8).
    pub fn invalid_proxy_path() -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            "invalid_proxy_path",
            "the proxied path has a `.` or `..` segment or a backslash",
        )
    }

    /// `X-Hypha-Cart` that isn't a cart ID (the app proxy, design note 0029, §8).
    pub fn invalid_cart_header() -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            "invalid_cart_header",
            "X-Hypha-Cart is a cart ID (`cart_…`)",
        )
    }

    /// A query string that doesn't parse, or has parameters the route doesn't take.
    pub fn invalid_query(message: impl std::fmt::Display) -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            "invalid_query",
            format!("invalid query string: {message}"),
        )
    }

    /// A list's `?cursor=` that isn't one the list handed out.
    pub fn invalid_cursor() -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            "invalid_cursor",
            "the cursor is not one this list returned",
        )
    }

    pub fn missing_content_type() -> Self {
        Self::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "missing_content_type",
            "expected a request with `Content-Type: application/json`",
        )
    }

    pub fn route_not_found() -> Self {
        Self::new(StatusCode::NOT_FOUND, "route_not_found", "no such route")
    }

    /// The asset routes on a host without file storage (`HYPHA_S3_ENDPOINT` unset).
    pub fn media_not_configured() -> Self {
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "media_not_configured",
            "this deployment has no file storage (HYPHA_S3_ENDPOINT)",
        )
    }

    /// The search engine failed: logged, and the client is told only that search is down
    /// (design note 0019), since the engine's message can name its internals.
    pub(crate) fn search_unavailable(error: impl std::fmt::Display) -> Self {
        tracing::error!(%error, "search engine failed");
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "search_unavailable",
            "search is unavailable; try again shortly",
        )
    }

    pub fn method_not_allowed() -> Self {
        Self::new(
            StatusCode::METHOD_NOT_ALLOWED,
            "method_not_allowed",
            "method not allowed for this route",
        )
    }

    pub fn internal(error: impl std::fmt::Display) -> Self {
        tracing::error!(%error, "internal error");
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "internal error",
        )
    }

    /// For routes whose URL names the resource: its "not found" (`code`) is a 404,
    /// not the 422 used when the same kind of ID arrives in a request body.
    pub fn not_found_if(mut self, code: &str) -> Self {
        if self.code == code {
            self.status = StatusCode::NOT_FOUND;
        }
        self
    }

    /// A domain error with the given status. Storage errors never reach clients.
    fn domain<E: ErrorCode + std::fmt::Display>(status: StatusCode, e: E) -> Self {
        Self::new(status, e.code(), e)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = Json(json!({
            "code": self.code,
            "error": self.message,
            "details": self.details,
        }));
        if self.status == StatusCode::UNAUTHORIZED {
            // RFC 9110: a 401 names the scheme the client should use.
            let challenge = [(axum::http::header::WWW_AUTHENTICATE, "Bearer")];
            return (self.status, challenge, body).into_response();
        }
        if let Some(secs) = self.retry_after_secs {
            let retry = [(axum::http::header::RETRY_AFTER, secs.to_string())];
            return (self.status, retry, body).into_response();
        }
        (self.status, body).into_response()
    }
}

// ---------- details shared by several crates' errors ----------

fn shortages(lines: impl IntoIterator<Item = (VariantId, u32, i64)>) -> Option<Value> {
    let lines: Vec<_> = lines
        .into_iter()
        .map(|(variant, requested, available)| {
            json!({ "variant": variant, "requested": requested, "available": available })
        })
        .collect();
    Some(json!({ "lines": lines }))
}

fn completed(order: OrderId) -> Option<Value> {
    Some(json!({ "order": order }))
}

// ---------- axum's own rejections (bad JSON, bad path IDs) ----------

impl From<JsonRejection> for ApiError {
    fn from(rejection: JsonRejection) -> Self {
        let code = match &rejection {
            JsonRejection::JsonSyntaxError(_) => "malformed_json",
            JsonRejection::MissingJsonContentType(_) => "missing_content_type",
            _ => "invalid_body",
        };
        Self::new(rejection.status(), code, rejection.body_text())
    }
}

impl From<PathRejection> for ApiError {
    fn from(rejection: PathRejection) -> Self {
        Self::new(rejection.status(), "invalid_path", rejection.body_text())
    }
}

// ---------- domain command errors ----------

/// A page size out of range (`?limit=`): checked by the host, which builds the
/// `PageLimit` a list command takes.
impl From<PageError> for ApiError {
    fn from(e: PageError) -> Self {
        Self::domain(StatusCode::UNPROCESSABLE_ENTITY, e)
    }
}

/// A country or currency in a query string (a list's filter) that isn't one.
impl From<CountryError> for ApiError {
    fn from(e: CountryError) -> Self {
        Self::domain(StatusCode::UNPROCESSABLE_ENTITY, e)
    }
}

impl From<CurrencyError> for ApiError {
    fn from(e: CurrencyError) -> Self {
        Self::domain(StatusCode::UNPROCESSABLE_ENTITY, e)
    }
}

/// A read outside a command (loading a product's assets for its response).
impl From<RepoError> for ApiError {
    fn from(e: RepoError) -> Self {
        match e {
            RepoError::Backend(_) => Self::internal(e),
            RepoError::StaleVersion | RepoError::Duplicate(_) | RepoError::Reference(_) => {
                Self::domain(StatusCode::CONFLICT, e)
            }
        }
    }
}

impl From<hypha_products::CommandError> for ApiError {
    fn from(e: hypha_products::CommandError) -> Self {
        use hypha_products::CommandError as E;
        let status = match &e {
            E::Auth(_) => StatusCode::FORBIDDEN,
            E::Product(p) => product_status(p),
            E::Money(_) => StatusCode::UNPROCESSABLE_ENTITY,
            E::HandleTaken { .. } | E::SkuTaken { .. } => StatusCode::CONFLICT,
            E::ProductNotFound(_) => StatusCode::NOT_FOUND,
            E::Repo(RepoError::Backend(_)) => return Self::internal(e),
            E::Repo(
                RepoError::StaleVersion | RepoError::Duplicate(_) | RepoError::Reference(_),
            ) => StatusCode::CONFLICT,
        };
        let details = match &e {
            E::Product(p) => product_details(p),
            E::SkuTaken { sku, product } => Some(json!({ "sku": sku, "product": product })),
            _ => None,
        };
        Self::domain(status, e).with_details(details)
    }
}

/// A product rule's status, for the products and merchandising routes.
fn product_status(e: &hypha_products::ProductError) -> StatusCode {
    use hypha_products::ProductError as P;
    match e {
        // Every route that names a variant names it in its URL.
        P::VariantNotFound(_) => StatusCode::NOT_FOUND,
        // What the product already has: other variants, their SKUs, its last variant.
        // Both ways to the same combination conflict alike (`duplicate_option_combination`).
        P::OptionValueInUse { .. }
        | P::VariantsCollide { .. }
        | P::DuplicateCombination
        | P::DuplicateSku(_)
        | P::NoVariants => StatusCode::CONFLICT,
        P::InvalidHandle(_)
        | P::Text(_)
        | P::DuplicateVariantId(_)
        | P::SelectionCountMismatch { .. }
        | P::UnknownOptionValue(_)
        | P::NoOptions
        | P::EmptyOption(_)
        | P::DuplicateOptionName(_)
        | P::DuplicateOptionValue(_)
        | P::InvalidTransition { .. }
        | P::InvalidPrice(_)
        | P::TooManyVariants { .. }
        | P::UnknownVariant(_)
        | P::UnknownOption(_)
        | P::DuplicateOption(_)
        | P::DuplicateOptionLabel { .. }
        | P::TooManyMedia { .. }
        | P::DuplicateMedia(_)
        | P::MediaNotInProduct(_)
        | P::DescriptionTooLong { .. }
        | P::TooManyTags { .. } => StatusCode::UNPROCESSABLE_ENTITY,
    }
}

fn product_details(e: &hypha_products::ProductError) -> Option<Value> {
    use hypha_products::ProductError as P;
    match e {
        P::OptionValueInUse { variants } => Some(json!({ "variants": variants })),
        P::VariantsCollide { first, second } => Some(json!({ "variants": [first, second] })),
        _ => None,
    }
}

impl From<hypha_merchandising::CommandError> for ApiError {
    fn from(e: hypha_merchandising::CommandError) -> Self {
        use hypha_merchandising::CommandError as E;
        let status = match &e {
            E::Auth(_) => StatusCode::FORBIDDEN,
            E::Product(p) => product_status(p),
            E::ProductNotFound(_) | E::HandleNotFound(_) => StatusCode::NOT_FOUND,
            // In the URL (`/admin/categories/{id}/image`).
            E::CategoryNotFound(_) => StatusCode::NOT_FOUND,
            // Assets arrive in the body.
            E::AssetNotFound(_)
            | E::AssetNotReady(_)
            | E::AssetNotPublic(_)
            | E::AssetKindNotAllowed { .. } => StatusCode::UNPROCESSABLE_ENTITY,
            E::Repo(RepoError::Backend(_)) => return Self::internal(e),
            E::Repo(
                RepoError::StaleVersion | RepoError::Duplicate(_) | RepoError::Reference(_),
            ) => StatusCode::CONFLICT,
        };
        let details = match &e {
            E::Product(p) => product_details(p),
            _ => None,
        };
        Self::domain(status, e).with_details(details)
    }
}

impl From<hypha_markets::CommandError> for ApiError {
    fn from(e: hypha_markets::CommandError) -> Self {
        use hypha_markets::CommandError as E;
        let status = match &e {
            E::Auth(_) => StatusCode::FORBIDDEN,
            E::CatalogNotFound(_) => StatusCode::NOT_FOUND,
            E::CountryInOtherMarket { .. } | E::CatalogExists { .. } => StatusCode::CONFLICT,
            E::Country(_)
            | E::Market(_)
            | E::Money(_)
            | E::PriceList(_)
            | E::PriceListNotFound(_)
            | E::MarketNotFound(_)
            | E::CurrencyMismatch { .. } => StatusCode::UNPROCESSABLE_ENTITY,
            E::Repo(RepoError::Backend(_)) => return Self::internal(e),
            E::Repo(
                RepoError::StaleVersion | RepoError::Duplicate(_) | RepoError::Reference(_),
            ) => StatusCode::CONFLICT,
        };
        Self::domain(status, e)
    }
}

impl From<hypha_stores::CommandError> for ApiError {
    fn from(e: hypha_stores::CommandError) -> Self {
        use hypha_stores::CommandError as E;
        let status = match &e {
            E::Auth(_) => StatusCode::FORBIDDEN,
            E::StoreNotFound(_) => StatusCode::NOT_FOUND,
            E::Store(_) | E::Email(_) | E::Address(_) | E::Timezone(_) | E::Language(_) => {
                StatusCode::UNPROCESSABLE_ENTITY
            }
            E::Repo(
                RepoError::StaleVersion | RepoError::Duplicate(_) | RepoError::Reference(_),
            ) => StatusCode::CONFLICT,
            // Only possible at startup, and a deployment problem rather than a request one.
            E::Repo(RepoError::Backend(_)) | E::MultipleStores(_) => return Self::internal(e),
        };
        Self::domain(status, e)
    }
}

impl From<hypha_categories::CommandError> for ApiError {
    fn from(e: hypha_categories::CommandError) -> Self {
        use hypha_categories::CommandError as E;
        let status = match &e {
            E::Auth(_) => StatusCode::FORBIDDEN,
            // In a body (a product's categories, a parent) unless a route says it's its
            // URL's; a cycle or a tree too deep is the placement asked for.
            E::Category(_) | E::CategoryNotFound(_) | E::ParentNotFound(_) => {
                StatusCode::UNPROCESSABLE_ENTITY
            }
            // A storefront's category by handle: always the URL's.
            E::HandleNotFound(_) => StatusCode::NOT_FOUND,
            E::HandleTaken(_) | E::HasChildren(_) => StatusCode::CONFLICT,
            E::Repo(RepoError::Backend(_)) => return Self::internal(e),
            E::Repo(
                RepoError::StaleVersion | RepoError::Duplicate(_) | RepoError::Reference(_),
            ) => StatusCode::CONFLICT,
        };
        Self::domain(status, e)
    }
}

impl From<hypha_channels::CommandError> for ApiError {
    fn from(e: hypha_channels::CommandError) -> Self {
        use hypha_channels::CommandError as E;
        let status = match &e {
            E::Auth(_) => StatusCode::FORBIDDEN,
            // In a body (a product's channels) unless a route says it's its URL's.
            E::Channel(_) | E::ChannelNotFound(_) => StatusCode::UNPROCESSABLE_ENTITY,
            E::NameTaken(_) => StatusCode::CONFLICT,
            E::Repo(RepoError::Backend(_)) => return Self::internal(e),
            E::Repo(
                RepoError::StaleVersion | RepoError::Duplicate(_) | RepoError::Reference(_),
            ) => StatusCode::CONFLICT,
        };
        Self::domain(status, e)
    }
}

impl From<hypha_search_index::CommandError> for ApiError {
    fn from(e: hypha_search_index::CommandError) -> Self {
        use hypha_search_index::CommandError as E;
        let status = match &e {
            E::Auth(_) => StatusCode::FORBIDDEN,
            E::Search(_) => StatusCode::UNPROCESSABLE_ENTITY,
            E::Index(error) => return Self::search_unavailable(error),
            E::Repo(RepoError::Backend(_)) => return Self::internal(e),
            E::Repo(
                RepoError::StaleVersion | RepoError::Duplicate(_) | RepoError::Reference(_),
            ) => StatusCode::CONFLICT,
        };
        Self::domain(status, e)
    }
}

impl From<hypha_search::CommandError> for ApiError {
    fn from(e: hypha_search::CommandError) -> Self {
        use hypha_search::CommandError as E;
        let status = match &e {
            E::Auth(_) => StatusCode::FORBIDDEN,
            E::Search(_) | E::Country(_) | E::NoMarketForCountry(_) => {
                StatusCode::UNPROCESSABLE_ENTITY
            }
            // A hidden category reads as missing (design note 0022, CA4).
            E::CategoryNotFound(_) => StatusCode::NOT_FOUND,
            E::Index(error) => return Self::search_unavailable(error),
            E::Repo(RepoError::Backend(_)) => return Self::internal(e),
            E::Repo(
                RepoError::StaleVersion | RepoError::Duplicate(_) | RepoError::Reference(_),
            ) => StatusCode::CONFLICT,
        };
        Self::domain(status, e)
    }
}

impl From<hypha_cart::CommandError> for ApiError {
    fn from(e: hypha_cart::CommandError) -> Self {
        use hypha_cart::{CartError, CommandError as E, LineError};
        let status = match &e {
            // A caller in no channel; storefront routes only take publishable keys, so
            // only commands called otherwise get here.
            E::Auth(_) | E::ChannelRequired => StatusCode::FORBIDDEN,
            E::CartNotFound(_) => StatusCode::NOT_FOUND,
            // Depends on the stock right now, not on the request: 409 like inventory's.
            E::Line(LineError::OutOfStock { .. }) => StatusCode::CONFLICT,
            // Checked out: the cart's state, not the request, is the problem.
            E::Cart(CartError::Completed { .. } | CartError::ShippingAddressRequired) => {
                StatusCode::CONFLICT
            }
            // Shipping (design note 0015): the group is in the URL; nothing to ship, no
            // zone and an option not offered right now are the cart's state.
            E::ShippingGroupNotFound(_) => StatusCode::NOT_FOUND,
            E::NothingToShip | E::NoShippingToCountry(_) | E::ShippingOptionNotOffered { .. } => {
                StatusCode::CONFLICT
            }
            E::Cart(_)
            | E::Address(_)
            | E::ShippingOptionUnknown(_)
            | E::Line(_)
            | E::Money(_)
            | E::MarketNotFound(_)
            | E::NoMarketForCountry(_)
            | E::VariantNotFound(_)
            | E::Email(_)
            | E::Country(_)
            | E::Idempotency(_)
            | E::CustomerNotFound(_) => StatusCode::UNPROCESSABLE_ENTITY,
            E::Repo(RepoError::Backend(_)) => return Self::internal(e),
            E::Repo(
                RepoError::StaleVersion | RepoError::Duplicate(_) | RepoError::Reference(_),
            ) => StatusCode::CONFLICT,
        };
        let details = match &e {
            E::Line(LineError::OutOfStock {
                variant,
                requested,
                available,
            }) => shortages([(*variant, *requested, *available)]),
            E::Cart(CartError::Completed { order }) => completed(*order),
            E::ShippingOptionNotOffered { reason, .. } => Some(json!({ "reason": reason.code() })),
            _ => None,
        };
        Self::domain(status, e).with_details(details)
    }
}

impl From<hypha_tax::CommandError> for ApiError {
    fn from(e: hypha_tax::CommandError) -> Self {
        use hypha_tax::{CommandError as E, TaxError};
        let status = match &e {
            E::Auth(_) => StatusCode::FORBIDDEN,
            // Region IDs only ever arrive in URLs; a class rate is addressed by its URL too.
            E::TaxRegionNotFound(_) | E::Tax(TaxError::ClassRateNotFound(_)) => {
                StatusCode::NOT_FOUND
            }
            E::TaxRegionExists { .. } => StatusCode::CONFLICT,
            E::Country(_) | E::Money(_) | E::Text(_) | E::TaxClassNotFound(_) => {
                StatusCode::UNPROCESSABLE_ENTITY
            }
            E::Repo(RepoError::Backend(_)) => return Self::internal(e),
            E::Repo(
                RepoError::StaleVersion | RepoError::Duplicate(_) | RepoError::Reference(_),
            ) => StatusCode::CONFLICT,
        };
        Self::domain(status, e)
    }
}

impl From<hypha_inventory::CommandError> for ApiError {
    fn from(e: hypha_inventory::CommandError) -> Self {
        use hypha_inventory::{CommandError as E, InventoryError as I};
        let status = match &e {
            E::Auth(_) => StatusCode::FORBIDDEN,
            // Item IDs only ever arrive in URLs; locations can come in either, so routes
            // that name one in the URL turn its 422 into a 404 (`not_found_if`).
            E::InventoryItemNotFound(_) => StatusCode::NOT_FOUND,
            E::LocationNotFound(_) => StatusCode::UNPROCESSABLE_ENTITY,
            E::InventoryItemExists { .. }
            | E::OutOfStock { .. }
            | E::Inventory(
                I::OnHandChanged { .. }
                | I::ReferenceInUse(_)
                | I::OutOfStock { .. }
                | I::StillReserved,
            ) => StatusCode::CONFLICT,
            E::Inventory(
                I::Text(_)
                | I::Address(_)
                | I::DuplicateMarket(_)
                | I::StockOutOfRange(_)
                | I::Quantity(_)
                | I::InvalidReference
                | I::DuplicateLine { .. },
            ) => StatusCode::UNPROCESSABLE_ENTITY,
            E::Repo(RepoError::Backend(_)) => return Self::internal(e),
            E::Repo(
                RepoError::StaleVersion | RepoError::Duplicate(_) | RepoError::Reference(_),
            ) => StatusCode::CONFLICT,
        };
        let details = match &e {
            E::OutOfStock { lines } => {
                shortages(lines.iter().map(|l| (l.variant, l.requested, l.available)))
            }
            _ => None,
        };
        Self::domain(status, e).with_details(details)
    }
}

impl From<hypha_customers::CommandError> for ApiError {
    fn from(e: hypha_customers::CommandError) -> Self {
        use hypha_customers::{CommandError as E, CustomerError as C};
        let status = match &e {
            E::Auth(_) => StatusCode::FORBIDDEN,
            // Customer and address IDs only ever arrive in URLs.
            E::CustomerNotFound(_) | E::Customer(C::AddressNotFound(_)) => StatusCode::NOT_FOUND,
            E::CustomerExists { .. } => StatusCode::CONFLICT,
            E::Email(_) | E::Address(_) | E::Customer(C::Text(_)) => {
                StatusCode::UNPROCESSABLE_ENTITY
            }
            // Address IDs are generated, so a clash is a bug, not a bad request.
            E::Customer(C::DuplicateAddress(_)) => return Self::internal(e),
            E::Repo(RepoError::Backend(_)) => return Self::internal(e),
            E::Repo(
                RepoError::StaleVersion | RepoError::Duplicate(_) | RepoError::Reference(_),
            ) => StatusCode::CONFLICT,
        };
        Self::domain(status, e)
    }
}

/// The status of a rule an order refused, or `None` for the rules only a bug or bad
/// stored data breaks (an order checkout built wrong), which are internal errors.
fn order_error_status(e: &hypha_orders::OrderError) -> Option<StatusCode> {
    use hypha_orders::OrderError as O;
    match e {
        // The state of the order in the URL.
        O::Cancelled(_) | O::HasFulfillments(_) | O::OverFulfilled { .. } => {
            Some(StatusCode::CONFLICT)
        }
        O::NotFulfilled { .. } => Some(StatusCode::CONFLICT),
        // A line ID in the body; a list's filter or sort.
        O::UnknownLine(_) | O::Filter(_) | O::SortNeedsCurrency => {
            Some(StatusCode::UNPROCESSABLE_ENTITY)
        }
        O::Text(_)
        | O::Money(_)
        | O::NoLines
        | O::DuplicateLine(_)
        | O::CurrencyMismatch { .. }
        | O::LineTotalMismatch(_)
        | O::TotalsMismatch
        | O::InvalidNumber(_)
        | O::ShippingWithoutAddress
        | O::UnknownShippedLine(_)
        | O::FulfilledPastQuantity(_)
        | O::FulfillmentStatusMismatch => None,
    }
}

impl From<hypha_orders::CommandError> for ApiError {
    fn from(e: hypha_orders::CommandError) -> Self {
        use hypha_orders::CommandError as E;
        let status = match &e {
            E::Auth(_) => StatusCode::FORBIDDEN,
            // Every order route names the order in its URL.
            E::OrderNotFound(_) => StatusCode::NOT_FOUND,
            E::Order(o) => match order_error_status(o) {
                Some(status) => status,
                None => return Self::internal(e),
            },
            E::Repo(RepoError::Backend(_)) => return Self::internal(e),
            E::Repo(
                RepoError::StaleVersion | RepoError::Duplicate(_) | RepoError::Reference(_),
            ) => StatusCode::CONFLICT,
        };
        Self::domain(status, e)
    }
}

/// The status of a fulfilment rule: bad input in the body (422), or a cancelled
/// fulfilment in the URL (409).
fn fulfillment_error_status(e: &hypha_fulfillment::FulfillmentError) -> StatusCode {
    use hypha_fulfillment::FulfillmentError as F;
    match e {
        F::Cancelled(_) => StatusCode::CONFLICT,
        F::Text(_)
        | F::NoLines
        | F::DuplicateLine(_)
        | F::TooMuchTracking { .. }
        | F::InvalidTrackingUrl(_) => StatusCode::UNPROCESSABLE_ENTITY,
    }
}

/// What a client acts on when it asked for more than a line has left.
fn order_details(e: &hypha_orders::OrderError) -> Option<Value> {
    match e {
        hypha_orders::OrderError::OverFulfilled {
            line,
            requested,
            unfulfilled,
        } => Some(json!({ "line": line, "requested": requested, "unfulfilled": unfulfilled })),
        _ => None,
    }
}

impl From<hypha_fulfillment::CommandError> for ApiError {
    fn from(e: hypha_fulfillment::CommandError) -> Self {
        use hypha_fulfillment::CommandError as E;
        let status = match &e {
            E::Auth(_) => StatusCode::FORBIDDEN,
            // Fulfilments only ever arrive in URLs.
            E::FulfillmentNotFound(_) => StatusCode::NOT_FOUND,
            E::Fulfillment(f) => fulfillment_error_status(f),
            E::Repo(RepoError::Backend(_)) => return Self::internal(e),
            E::Repo(
                RepoError::StaleVersion | RepoError::Duplicate(_) | RepoError::Reference(_),
            ) => StatusCode::CONFLICT,
        };
        Self::domain(status, e)
    }
}

impl From<hypha_order_actions::CommandError> for ApiError {
    fn from(e: hypha_order_actions::CommandError) -> Self {
        use hypha_inventory::InventoryError as I;
        use hypha_order_actions::CommandError as E;
        let status = match &e {
            E::Auth(_) => StatusCode::FORBIDDEN,
            // The order, fulfilment or payment in the URL; the location comes in the body.
            E::OrderNotFound(_) | E::FulfillmentNotFound(_) | E::PaymentNotFound(_) => {
                StatusCode::NOT_FOUND
            }
            E::LocationNotFound(_) | E::AmountMismatch { .. } | E::RefundReferenceRequired(_) => {
                StatusCode::UNPROCESSABLE_ENTITY
            }
            // Another provider's payment: this app may not report on it.
            E::PaymentNotYours { .. } => StatusCode::FORBIDDEN,
            // The order's or its payments' state.
            E::HasFulfillments { .. } | E::NotPayable { .. } | E::PaymentAuthorized { .. } => {
                StatusCode::CONFLICT
            }
            E::Order(o) => match order_error_status(o) {
                Some(status) => status,
                None => return Self::internal(e),
            },
            E::Fulfillment(f) => fulfillment_error_status(f),
            E::Payment(p) => match payment_error_status(p) {
                Some(status) => status,
                None => return Self::internal(e),
            },
            E::Quantity(_) | E::Idempotency(_) => StatusCode::UNPROCESSABLE_ENTITY,
            // A count that would leave the representable range: the stock's state.
            E::Inventory(I::StockOutOfRange(_)) => StatusCode::CONFLICT,
            // Order line references and deletions are built by the workflow: a bug.
            E::Inventory(
                I::Text(_)
                | I::Address(_)
                | I::DuplicateMarket(_)
                | I::OnHandChanged { .. }
                | I::Quantity(_)
                | I::InvalidReference
                | I::ReferenceInUse(_)
                | I::OutOfStock { .. }
                | I::DuplicateLine { .. }
                | I::StillReserved,
            ) => return Self::internal(e),
            E::Repo(RepoError::Backend(_)) => return Self::internal(e),
            E::Repo(
                RepoError::StaleVersion | RepoError::Duplicate(_) | RepoError::Reference(_),
            ) => StatusCode::CONFLICT,
        };
        let details = match &e {
            E::HasFulfillments { fulfillments, .. } => {
                Some(json!({ "fulfillments": fulfillments }))
            }
            E::NotPayable { status, .. } => Some(json!({ "financial_status": status.as_str() })),
            E::PaymentAuthorized { payment, .. } => Some(json!({ "payment": payment })),
            E::Order(o) => order_details(o),
            E::Payment(p) => payment_details(p),
            _ => None,
        };
        Self::domain(status, e).with_details(details)
    }
}

impl From<hypha_media::BlobError> for ApiError {
    /// File storage failing is ours to fix, like a database failure: logged, not shown.
    fn from(e: hypha_media::BlobError) -> Self {
        Self::internal(e)
    }
}

impl From<hypha_media::CommandError> for ApiError {
    fn from(e: hypha_media::CommandError) -> Self {
        use hypha_media::{CommandError as E, MediaError as M};
        let status = match &e {
            E::Auth(_) => StatusCode::FORBIDDEN,
            // Every asset route names the asset in its URL.
            E::AssetNotFound(_) => StatusCode::NOT_FOUND,
            // The asset's state: not uploaded yet, or not pending any more.
            E::UploadMissing(_) | E::Media(M::NotPending { .. }) | E::AssetInUse { .. } => {
                StatusCode::CONFLICT
            }
            E::Media(
                M::Text(_)
                | M::UnsupportedMediaType(_)
                | M::EmptyFile
                | M::FileTooLarge { .. }
                | M::InvalidChecksum(_)
                | M::InvalidOwnerKind(_)
                | M::InvalidOwnerId(_)
                | M::InvalidRole(_)
                | M::InvalidOwner(_),
            )
            | E::UploadMismatch(_) => StatusCode::UNPROCESSABLE_ENTITY,
            E::Blob(_) | E::Repo(RepoError::Backend(_)) => return Self::internal(e),
            E::Repo(
                RepoError::StaleVersion | RepoError::Duplicate(_) | RepoError::Reference(_),
            ) => StatusCode::CONFLICT,
        };
        let details = match &e {
            E::AssetInUse {
                usages, has_more, ..
            } => {
                let usages: Vec<Value> = usages
                    .iter()
                    .map(|u| {
                        json!({
                            "owner": { "kind": u.owner.kind(), "id": u.owner.id() },
                            "role": u.role.as_str(),
                        })
                    })
                    .collect();
                Some(json!({ "usages": usages, "has_more": has_more }))
            }
            _ => None,
        };
        Self::domain(status, e).with_details(details)
    }
}

/// A payment rule's status (design note 0027), or `None` for one only a bug or bad stored
/// data breaks (a 500).
fn payment_error_status(e: &hypha_payments::PaymentError) -> Option<StatusCode> {
    use hypha_payments::PaymentError as P;
    match e {
        // What staff typed: a method's name, instructions, provider, settings, a reference.
        P::Text(_)
        | P::InvalidProviderKey(_)
        | P::UnknownProvider(_)
        | P::InvalidSettings { .. }
        | P::InstructionsTooLong => Some(StatusCode::UNPROCESSABLE_ENTITY),
        // The payment's state, not the request; or its provider, which takes no actions.
        P::InvalidTransition { .. } | P::ActionUnsupported { .. } => Some(StatusCode::CONFLICT),
        // A refund that doesn't fit what was paid.
        P::InvalidRefund { .. } => Some(StatusCode::UNPROCESSABLE_ENTITY),
        // A provider that couldn't do what was asked.
        P::ProviderFailed { .. } | P::AppUnavailable { .. } => Some(StatusCode::BAD_GATEWAY),
        // An open payment whose method was deleted (only a checkout replay meets it, and
        // logs it): the state of the store, not the request.
        P::MethodUnavailable(_) => Some(StatusCode::CONFLICT),
        P::Money(_) | P::CurrencyMismatch { .. } | P::Inconsistent { .. } => None,
    }
}

/// What a client acts on in a payment error: the app and point that failed.
fn payment_details(e: &hypha_payments::PaymentError) -> Option<Value> {
    match e {
        hypha_payments::PaymentError::AppUnavailable { app, point, .. } => {
            Some(json!({ "app": app.as_str(), "point": point }))
        }
        _ => None,
    }
}

impl From<hypha_payments::CommandError> for ApiError {
    fn from(e: hypha_payments::CommandError) -> Self {
        use hypha_payments::CommandError as E;
        let status = match &e {
            E::Auth(_) => StatusCode::FORBIDDEN,
            // Routes that name the method in their URL say so (`not_found_if`).
            E::PaymentMethodNotFound(_) => StatusCode::UNPROCESSABLE_ENTITY,
            // Only routes with the payment in the URL look one up.
            E::PaymentNotFound(_) => StatusCode::NOT_FOUND,
            E::PaymentMethodInUse(_) | E::PaymentRefused { .. } => StatusCode::CONFLICT,
            E::Payment(p) => match payment_error_status(p) {
                Some(status) => status,
                None => return Self::internal(e),
            },
            E::Repo(RepoError::Backend(_)) => return Self::internal(e),
            E::Repo(
                RepoError::StaleVersion | RepoError::Duplicate(_) | RepoError::Reference(_),
            ) => StatusCode::CONFLICT,
        };
        let details = match &e {
            E::PaymentRefused { reason, .. } => Some(json!({ "reason": reason })),
            E::Payment(p) => payment_details(p),
            _ => None,
        };
        Self::domain(status, e).with_details(details)
    }
}

impl From<hypha_checkout::CommandError> for ApiError {
    fn from(e: hypha_checkout::CommandError) -> Self {
        use hypha_checkout::CommandError as E;
        let status = match &e {
            E::Auth(_) => StatusCode::FORBIDDEN,
            // Pricing the cart and reserving stock report like their own routes.
            E::Cart(inner) => return Self::from(inner.clone()),
            E::Inventory(inner) => return Self::from(inner.clone()),
            // Asking to pay for a cancelled order.
            E::Order(hypha_orders::OrderError::Cancelled(_)) => StatusCode::CONFLICT,
            // Checkout builds the order from a priced cart, so a broken order rule is a bug;
            // likewise the payment, from a stored method.
            E::Order(_) | E::Payment(_) => return Self::internal(e),
            // A method in a body (`PUT …/payment-method`).
            E::PaymentMethodNotFound(_) => StatusCode::UNPROCESSABLE_ENTITY,
            E::Idempotency(_) => StatusCode::UNPROCESSABLE_ENTITY,
            // The cart or the order in the URL.
            E::CartNotFound(_) | E::OrderNotFound(_) => StatusCode::NOT_FOUND,
            // The cart's or the shop's state right now, not the request, is the problem.
            E::CartCompleted { .. }
            | E::EmptyCart(_)
            | E::MissingEmail(_)
            | E::Unavailable { .. }
            | E::TotalChanged { .. }
            | E::ShippingAddressRequired(_)
            | E::ShippingOptionRequired(_)
            | E::ShippingUnavailable(_)
            | E::PaymentMethodRequired(_)
            | E::PaymentMethodUnavailable(_)
            | E::NotPayable { .. } => StatusCode::CONFLICT,
            E::Repo(RepoError::Backend(_)) => return Self::internal(e),
            E::Repo(
                RepoError::StaleVersion | RepoError::Duplicate(_) | RepoError::Reference(_),
            ) => StatusCode::CONFLICT,
        };
        let details = match &e {
            E::CartCompleted { order, .. } => completed(*order),
            E::Unavailable { lines } => Some(json!({ "lines": lines })),
            E::ShippingUnavailable(problem) => Some(json!({ "reason": problem.code() })),
            E::TotalChanged { expected, actual } => {
                Some(json!({ "expected": expected, "actual": actual }))
            }
            E::NotPayable { status, .. } => Some(json!({ "financial_status": status.as_str() })),
            _ => None,
        };
        Self::domain(status, e).with_details(details)
    }
}

/// An app's extension point gave no usable answer (design note 0029, §15): 502
/// `app_unavailable` with the app and point. Why (a timeout, a status, a malformed
/// answer) is in the call log for staff, not in a response a shopper may see.
impl From<hypha_extensions::ExtensionError> for ApiError {
    fn from(e: hypha_extensions::ExtensionError) -> Self {
        let message = format!("the app {} couldn't answer {}", e.app(), e.point());
        Self::new(StatusCode::BAD_GATEWAY, e.code(), message)
            .with_details(Some(json!({ "app": e.app().as_str(), "point": e.point() })))
    }
}

impl From<hypha_extensions::CommandError> for ApiError {
    fn from(e: hypha_extensions::CommandError) -> Self {
        use hypha_extensions::CommandError as E;
        let status = match &e {
            E::Auth(_) => StatusCode::FORBIDDEN,
            E::Repo(RepoError::Backend(_)) => return Self::internal(e),
            E::Repo(
                RepoError::StaleVersion | RepoError::Duplicate(_) | RepoError::Reference(_),
            ) => StatusCode::CONFLICT,
        };
        Self::domain(status, e)
    }
}

impl From<hypha_apps::CommandError> for ApiError {
    fn from(e: hypha_apps::CommandError) -> Self {
        use hypha_apps::CommandError as E;
        let status = match &e {
            E::Auth(_) => StatusCode::FORBIDDEN,
            // Apps, deliveries and events only ever arrive in URLs (a malformed key in
            // one is `AppNotFound` too); a new app's key in a body is a conflict when
            // taken, and malformed is `App(Key(_))`, 422.
            E::AppNotFound(_)
            | E::DeliveryNotFound { .. }
            | E::EventNotFound(_)
            | E::ExtensionNotFound { .. }
            | E::ProxyNotFound(_) => StatusCode::NOT_FOUND,
            E::AppExists(_) | E::ActionHasPoint { .. } => StatusCode::CONFLICT,
            E::InvalidContext(_) => StatusCode::UNPROCESSABLE_ENTITY,
            E::App(inner) => return Self::from_app(inner.clone()),
            E::Repo(RepoError::Backend(_)) => return Self::internal(e),
            E::Repo(
                RepoError::StaleVersion | RepoError::Duplicate(_) | RepoError::Reference(_),
            ) => StatusCode::CONFLICT,
        };
        Self::domain(status, e)
    }
}

impl ApiError {
    /// An app's own rules (design notes 0016, 0029): bad input is 422, with every
    /// manifest problem in `details.errors` (`[{"path", "message"}]`); an app that isn't
    /// installed through OAuth, for uninstalling or narrowing, and staff setting the
    /// endpoints of one whose manifest registers them, 409.
    fn from_app(e: hypha_apps::AppError) -> Self {
        use hypha_apps::AppError as E;
        let status = match &e {
            E::NotInstalled | E::EndpointsFromManifest(_) => StatusCode::CONFLICT,
            E::IncompleteManifest => return Self::internal(e),
            _ => StatusCode::UNPROCESSABLE_ENTITY,
        };
        let details = match &e {
            E::Manifest(m) => Some(manifest_problems(m)),
            _ => None,
        };
        Self::domain(status, e).with_details(details)
    }

    /// A manifest that couldn't be fetched (`manifest_unreachable`) or isn't valid
    /// (`manifest_invalid`, `details.errors`), at `status`.
    pub fn manifest(status: StatusCode, e: hypha_apps::FetchError) -> Self {
        let details = match &e {
            hypha_apps::FetchError::Invalid(m) => Some(manifest_problems(m)),
            hypha_apps::FetchError::Unreachable { .. } => None,
        };
        Self::domain(status, e).with_details(details)
    }

    /// An OAuth endpoint's error that can't go back to the app (RFC 6749 §4.1.2.1): a
    /// 400 with the usual body, for the browser.
    pub fn unredirectable(e: hypha_oauth::UnredirectableError) -> Self {
        match e {
            hypha_oauth::UnredirectableError::Manifest(m) => {
                Self::manifest(StatusCode::BAD_REQUEST, m)
            }
            other => Self::domain(StatusCode::BAD_REQUEST, other),
        }
    }
}

fn manifest_problems(e: &hypha_apps::ManifestError) -> Value {
    json!({
        "errors": e.problems.iter().map(|p| json!({"path": p.path, "message": p.message}))
            .collect::<Vec<_>>()
    })
}

impl From<hypha_oauth::CommandError> for ApiError {
    fn from(e: hypha_oauth::CommandError) -> Self {
        use hypha_oauth::CommandError as E;
        let status = match &e {
            E::Auth(_) | E::CannotGrant(_) => StatusCode::FORBIDDEN,
            E::App(inner) => return Self::from_app(inner.clone()),
            // Refreshing (§15: 422 with `details.errors` when invalid).
            E::Manifest(inner) => {
                return Self::manifest(StatusCode::UNPROCESSABLE_ENTITY, inner.clone());
            }
            E::InvalidToken => return hypha_auth::CommandError::InvalidToken.into(),
            // Requests and apps only arrive in URLs.
            E::AppNotFound(_) | E::RequestNotFound(_) => StatusCode::NOT_FOUND,
            E::RequestExpired(_)
            | E::RequestDecided(_)
            | E::PrivateApp(_)
            | E::InstalledFromUrl(_)
            | E::InstalledFromOtherUrl(_) => StatusCode::CONFLICT,
            E::Seal(_) | E::Repo(RepoError::Backend(_)) => return Self::internal(e),
            E::Repo(
                RepoError::StaleVersion | RepoError::Duplicate(_) | RepoError::Reference(_),
            ) => StatusCode::CONFLICT,
        };
        Self::domain(status, e)
    }
}

impl From<hypha_shipping::CommandError> for ApiError {
    fn from(e: hypha_shipping::CommandError) -> Self {
        use hypha_shipping::{CommandError as E, ProviderError as P};
        let status = match &e {
            E::Auth(_) => StatusCode::FORBIDDEN,
            // Zones and providers only ever arrive in URLs; options in a URL are mapped
            // by their routes (`not_found_if`), in a body (reordering) they're 422.
            E::ZoneNotFound(_) | E::ProviderNotFound(_) | E::Provider(P::Unknown(_)) => {
                StatusCode::NOT_FOUND
            }
            E::CountryInOtherZone { .. } | E::ProviderExists(_) | E::Provider(P::Disabled(_)) => {
                StatusCode::CONFLICT
            }
            // The app answered badly or not at all: the request was fine.
            E::Provider(P::Timeout(_) | P::Failed { .. } | P::WrongCurrency { .. }) => {
                StatusCode::BAD_GATEWAY
            }
            // The key is in the body when creating.
            E::Shipping(_) | E::Country(_) | E::Money(_) | E::ProviderAppNotFound(_) => {
                StatusCode::UNPROCESSABLE_ENTITY
            }
            E::Repo(RepoError::Backend(_)) => return Self::internal(e),
            E::Repo(
                RepoError::StaleVersion | RepoError::Duplicate(_) | RepoError::Reference(_),
            ) => StatusCode::CONFLICT,
        };
        Self::domain(status, e)
    }
}

/// A template problem's place and words (`template_invalid`'s `details`).
fn template_problem(problem: Option<&hypha_notifications::TemplateProblem>) -> Option<Value> {
    problem.map(|p| json!({ "part": p.part.as_str(), "line": p.line, "message": p.message }))
}

fn template_error_status(e: &hypha_notifications::TemplateError) -> StatusCode {
    use hypha_notifications::TemplateError as T;
    match e {
        // Template keys only ever arrive in URLs (a list's filter is parsed as a query).
        T::UnknownKey(_) => StatusCode::NOT_FOUND,
        // Never allowed for that template, whatever its state: invalid input.
        T::Invalid(_) | T::TooLarge { .. } | T::Missing(_) | T::NotInLayout(_) | T::Required(_) => {
            StatusCode::UNPROCESSABLE_ENTITY
        }
    }
}

/// A message's record: a move its status doesn't allow is the state of the message;
/// stored data that breaks its rules is the server's.
fn notification_error_status(e: &hypha_notifications::NotificationError) -> Option<StatusCode> {
    use hypha_notifications::NotificationError as N;
    match e {
        N::Text(_) => Some(StatusCode::UNPROCESSABLE_ENTITY),
        N::InvalidTransition { .. } | N::NotDue(_) => Some(StatusCode::CONFLICT),
        N::Inconsistent { .. } => None,
    }
}

impl From<hypha_notifications::CommandError> for ApiError {
    fn from(e: hypha_notifications::CommandError) -> Self {
        use hypha_notifications::{CommandError as E, TemplateError as T};
        let status = match &e {
            E::Auth(_) => StatusCode::FORBIDDEN,
            E::Template(t) => template_error_status(t),
            E::Settings(_) => StatusCode::UNPROCESSABLE_ENTITY,
            E::Notification(n) => match notification_error_status(n) {
                Some(status) => status,
                None => return Self::internal(e),
            },
            // Message IDs only ever arrive in URLs.
            E::NotificationNotFound(_) => StatusCode::NOT_FOUND,
            E::Repo(RepoError::Backend(_)) => return Self::internal(e),
            E::Repo(
                RepoError::StaleVersion | RepoError::Duplicate(_) | RepoError::Reference(_),
            ) => StatusCode::CONFLICT,
        };
        let details = match &e {
            E::Template(T::Invalid(problem)) => template_problem(Some(problem)),
            _ => None,
        };
        Self::domain(status, e).with_details(details)
    }
}

impl From<hypha_mail::CommandError> for ApiError {
    fn from(e: hypha_mail::CommandError) -> Self {
        use hypha_mail::CommandError as E;
        let status = match &e {
            E::Auth(_) => StatusCode::FORBIDDEN,
            // A test send by an API key: only a staff member has an address of their own.
            E::StaffOnly => StatusCode::FORBIDDEN,
            E::Template(t) => template_error_status(t),
            E::Render(_) | E::Settings(_) => StatusCode::UNPROCESSABLE_ENTITY,
            E::Notification(n) => match notification_error_status(n) {
                Some(status) => status,
                None => return Self::internal(e),
            },
            // The provider didn't take it (unreachable, or refused it for good): like
            // the staff flows' `notification_failed`, the upstream's answer.
            E::Send(_) => StatusCode::BAD_GATEWAY,
            // A preview's order (and its fulfilments), and the logo, arrive in the body.
            E::OrderNotFound(_)
            | E::FulfillmentNotFound(_)
            | E::NoFulfillment(_)
            | E::AssetNotFound(_)
            | E::AssetNotReady(_)
            | E::AssetNotPublic(_)
            | E::AssetKindNotAllowed { .. } => StatusCode::UNPROCESSABLE_ENTITY,
            // Message IDs only ever arrive in URLs.
            E::NotificationNotFound(_) => StatusCode::NOT_FOUND,
            // The deployment, not the request: a test send with the `log` sender.
            E::EmailNotConfigured => StatusCode::CONFLICT,
            // The message in the URL is an invite or a reset.
            E::NotResendable { .. } => StatusCode::CONFLICT,
            // A host set up wrongly, or an event the store wrote itself.
            E::StoreNotFound(_) | E::BadEvent { .. } => return Self::internal(e),
            E::Repo(RepoError::Backend(_)) => return Self::internal(e),
            E::Repo(
                RepoError::StaleVersion | RepoError::Duplicate(_) | RepoError::Reference(_),
            ) => StatusCode::CONFLICT,
        };
        let details = template_problem(e.problem());
        Self::domain(status, e).with_details(details)
    }
}

impl From<hypha_auth::CommandError> for ApiError {
    fn from(e: hypha_auth::CommandError) -> Self {
        use hypha_auth::{ApiKeyError as K, CommandError as E, IdentityError as I};
        let status = match &e {
            E::Auth(_) | E::CannotGrant(_) => StatusCode::FORBIDDEN,
            E::InvalidCredentials | E::InvalidToken | E::InvalidMfaCode => StatusCode::UNAUTHORIZED,
            E::MfaLocked { .. } => StatusCode::TOO_MANY_REQUESTS,
            // Only on `/me` routes, by someone logged in.
            E::WrongPassword => StatusCode::UNPROCESSABLE_ENTITY,
            // Key and session IDs only ever arrive in URLs.
            E::ApiKeyNotFound(_) | E::SessionNotFound(_) => StatusCode::NOT_FOUND,
            E::ApiKeyRevoked(_)
            | E::Identity(
                I::MfaAlreadyEnabled | I::MfaNotEnabled | I::MfaSetupNotStarted | I::NoPassword,
            ) => StatusCode::CONFLICT,
            E::Password(_)
            | E::ApiKey(
                K::Text(_)
                | K::NoScopes
                | K::PublishableScopesFixed
                | K::ChannelRequired
                | K::ChannelNotAllowed,
            )
            // A new key's channel arrives in the body.
            | E::ChannelNotFound(_) => StatusCode::UNPROCESSABLE_ENTITY,
            // A session whose identity is gone, a sealed secret that won't open, a hasher
            // failure: all server-side.
            E::IdentityNotFound(_) | E::Hash(_) | E::Seal(_) => return Self::internal(e),
            E::Repo(RepoError::Backend(_)) => return Self::internal(e),
            E::Repo(
                RepoError::StaleVersion | RepoError::Duplicate(_) | RepoError::Reference(_),
            ) => StatusCode::CONFLICT,
        };
        let details = match &e {
            E::MfaLocked { until } => crate::routes::rfc3339(*until)
                .ok()
                .map(|until| json!({ "locked_until": until })),
            _ => None,
        };
        Self::domain(status, e).with_details(details)
    }
}

impl From<hypha_users::CommandError> for ApiError {
    fn from(e: hypha_users::CommandError) -> Self {
        use hypha_users::{CommandError as E, RoleError as R, UserError as U};
        let status = match &e {
            E::Login(inner) => return Self::from(inner.clone()),
            E::Auth(_) => StatusCode::FORBIDDEN,
            E::NotStaff => StatusCode::UNAUTHORIZED,
            // User IDs only ever arrive in URLs; role IDs arrive in either, so routes
            // naming a role in the URL turn its 422 into a 404 (`not_found_if`). Invite
            // IDs only arrive in URLs.
            E::UserNotFound(_) | E::InviteNotFound(_) => StatusCode::NOT_FOUND,
            E::RoleNotFound(_) => StatusCode::UNPROCESSABLE_ENTITY,
            E::UserExists(_) | E::RoleNameTaken(_) | E::LastOwner | E::Role(R::OwnerRoleFixed) => {
                StatusCode::CONFLICT
            }
            E::Role(R::Text(_) | R::NoScopes)
            | E::User(U::Text(_))
            | E::Email(_)
            | E::Password(_) => StatusCode::UNPROCESSABLE_ENTITY,
            E::Notify(_) => StatusCode::BAD_GATEWAY,
            E::Repo(RepoError::Backend(_)) => return Self::internal(e),
            E::Repo(
                RepoError::StaleVersion | RepoError::Duplicate(_) | RepoError::Reference(_),
            ) => StatusCode::CONFLICT,
        };
        if let E::Notify(inner) = &e {
            // The provider's error stays in the log; the client learns what to do.
            tracing::error!(error = %inner, "email not sent");
            return Self::new(
                status,
                e.code(),
                "saved, but the email could not be sent; try again to resend it",
            );
        }
        Self::domain(status, e)
    }
}

#[cfg(test)]
mod tests {
    use hypha_core::{AuthError, CartId, Scope};
    use uuid::Uuid;

    use super::*;

    #[test]
    fn an_app_that_fails_its_point_is_a_bad_gateway_naming_both() {
        let e = ApiError::from(hypha_extensions::ExtensionError::Unavailable {
            app: hypha_core::AppKey::new("app_stripe").unwrap(),
            point: "payments.start",
            reason: "answered HTTP 401: invalid_signature".to_string(),
        });
        assert_eq!(e.status, StatusCode::BAD_GATEWAY);
        assert_eq!(e.code, "app_unavailable");
        assert_eq!(
            e.details,
            Some(json!({"app": "app_stripe", "point": "payments.start"}))
        );
        // The reason is the call log's, not the shopper's.
        assert_eq!(
            e.message,
            "the app app_stripe couldn't answer payments.start"
        );
    }

    #[test]
    fn domain_error_keeps_status_code_and_message() {
        let id = CartId::from_uuid(Uuid::from_u128(1));
        let e = ApiError::from(hypha_cart::CommandError::CartNotFound(id));
        assert_eq!(e.status, StatusCode::NOT_FOUND);
        assert_eq!(e.code, "cart_not_found");
        assert_eq!(e.message, format!("cart {id} not found"));
    }

    #[test]
    fn wrapped_error_uses_inner_code() {
        let e = ApiError::from(hypha_markets::CommandError::Auth(AuthError::MissingScope(
            Scope::PricingWrite,
        )));
        assert_eq!(e.status, StatusCode::FORBIDDEN);
        assert_eq!(e.code, "missing_scope");
        assert_eq!(e.message, "missing scope pricing:write");
    }

    #[test]
    fn a_reused_idempotency_key_is_invalid_input() {
        let e = ApiError::from(hypha_cart::CommandError::Idempotency(
            hypha_core::IdempotencyError::KeyReused,
        ));
        assert_eq!(e.status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(e.code, "idempotency_key_reused");
    }

    #[test]
    fn checking_out_twice_is_a_conflict() {
        let e = ApiError::from(hypha_checkout::CommandError::CartCompleted {
            cart: CartId::from_uuid(Uuid::from_u128(1)),
            order: hypha_core::OrderId::from_uuid(Uuid::from_u128(2)),
        });
        assert_eq!(e.status, StatusCode::CONFLICT);
        assert_eq!(e.code, "cart_completed");
        assert_eq!(
            e.details,
            Some(json!({ "order": "ord_00000000000000000000000000000002" }))
        );
    }

    #[test]
    fn an_empty_cart_or_one_without_email_is_a_conflict() {
        let cart = CartId::from_uuid(Uuid::from_u128(1));
        for e in [
            hypha_checkout::CommandError::EmptyCart(cart),
            hypha_checkout::CommandError::MissingEmail(cart),
        ] {
            assert_eq!(ApiError::from(e).status, StatusCode::CONFLICT);
        }
    }

    #[test]
    fn checkout_errors_carry_their_data_in_details() {
        let eur = |n| hypha_core::Money::new(n, hypha_core::Currency::EUR).unwrap();
        let e = ApiError::from(hypha_checkout::CommandError::TotalChanged {
            expected: eur(1000),
            actual: eur(1200),
        });
        assert_eq!(
            e.details,
            Some(json!({
                "expected": { "amount": 1000, "currency": "EUR" },
                "actual": { "amount": 1200, "currency": "EUR" },
            }))
        );

        let variant = VariantId::from_uuid(Uuid::from_u128(3));
        let e = ApiError::from(hypha_checkout::CommandError::Unavailable {
            lines: vec![variant],
        });
        assert_eq!(e.details, Some(json!({ "lines": [variant] })));

        let e = ApiError::from(hypha_checkout::CommandError::Inventory(
            hypha_inventory::CommandError::OutOfStock {
                lines: vec![hypha_inventory::Shortage {
                    variant,
                    requested: 3,
                    available: 1,
                }],
            },
        ));
        assert_eq!(
            e.details,
            Some(json!({ "lines": [{ "variant": variant, "requested": 3, "available": 1 }] }))
        );
    }

    #[test]
    fn errors_without_data_have_null_details() {
        let e = ApiError::route_not_found();
        assert_eq!(e.details, None);
    }

    #[test]
    fn checkout_reports_a_shortage_like_inventory() {
        let e = ApiError::from(hypha_checkout::CommandError::Inventory(
            hypha_inventory::CommandError::OutOfStock { lines: vec![] },
        ));
        assert_eq!(e.status, StatusCode::CONFLICT);
        assert_eq!(e.code, "out_of_stock");
    }

    #[test]
    fn an_unknown_order_in_the_url_is_not_found() {
        let id = hypha_core::OrderId::from_uuid(Uuid::from_u128(1));
        let e = ApiError::from(hypha_orders::CommandError::OrderNotFound(id));
        assert_eq!(e.status, StatusCode::NOT_FOUND);
        assert_eq!(e.code, "order_not_found");
    }

    #[test]
    fn stale_version_is_a_conflict() {
        let e = ApiError::from(hypha_cart::CommandError::Repo(RepoError::StaleVersion));
        assert_eq!(e.status, StatusCode::CONFLICT);
        assert_eq!(e.code, "concurrent_modification");
    }

    #[test]
    fn moved_stock_count_is_a_conflict() {
        let e = ApiError::from(hypha_inventory::CommandError::Inventory(
            hypha_inventory::InventoryError::OnHandChanged {
                expected: 5,
                actual: 4,
            },
        ));
        assert_eq!(e.status, StatusCode::CONFLICT);
        assert_eq!(e.code, "on_hand_changed");
    }

    #[test]
    fn out_of_stock_from_a_cart_is_a_conflict() {
        let variant = hypha_core::VariantId::from_uuid(Uuid::from_u128(1));
        let e = ApiError::from(hypha_cart::CommandError::Line(
            hypha_cart::LineError::OutOfStock {
                variant,
                requested: 3,
                available: 2,
            },
        ));
        assert_eq!((e.status, e.code), (StatusCode::CONFLICT, "out_of_stock"));
    }

    #[test]
    fn location_in_a_body_is_invalid_input_and_in_a_url_not_found() {
        let id = hypha_core::LocationId::from_uuid(Uuid::from_u128(1));
        let e = ApiError::from(hypha_inventory::CommandError::LocationNotFound(id));
        assert_eq!(e.status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            e.not_found_if("location_not_found").status,
            StatusCode::NOT_FOUND
        );
    }

    #[test]
    fn taken_email_is_a_conflict_and_unknown_customer_not_found() {
        let id = hypha_core::CustomerId::from_uuid(Uuid::from_u128(1));
        let e = ApiError::from(hypha_customers::CommandError::CustomerExists {
            email: hypha_core::Email::new("ana@example.hr").unwrap(),
            customer: id,
        });
        assert_eq!(
            (e.status, e.code),
            (StatusCode::CONFLICT, "customer_exists")
        );
        let e = ApiError::from(hypha_customers::CommandError::CustomerNotFound(id));
        assert_eq!(e.status, StatusCode::NOT_FOUND);
    }

    #[test]
    fn an_invalid_template_says_where_in_details() {
        use hypha_notifications::{RenderError, TemplatePart, TemplateProblem};
        let e = ApiError::from(hypha_mail::CommandError::Render(RenderError::Invalid(
            TemplateProblem {
                part: TemplatePart::Html,
                line: Some(3),
                message: "undefined value".to_string(),
            },
        )));
        assert_eq!(
            (e.status, e.code),
            (StatusCode::UNPROCESSABLE_ENTITY, "template_invalid")
        );
        assert_eq!(
            e.details,
            Some(json!({ "part": "html", "line": 3, "message": "undefined value" }))
        );
    }

    #[test]
    fn an_email_the_provider_didnt_take_is_a_bad_gateway() {
        use hypha_notifications::SendError;
        for (error, code) in [
            (
                SendError::Transient("timeout".to_string()),
                "email_unavailable",
            ),
            (
                SendError::Permanent("no such domain".to_string()),
                "email_refused",
            ),
        ] {
            let e = ApiError::from(hypha_mail::CommandError::Send(error));
            assert_eq!((e.status, e.code), (StatusCode::BAD_GATEWAY, code));
        }
        let e = ApiError::from(hypha_mail::CommandError::EmailNotConfigured);
        assert_eq!(
            (e.status, e.code),
            (StatusCode::CONFLICT, "email_not_configured")
        );
    }

    #[test]
    fn storage_error_hides_message() {
        let e = ApiError::from(hypha_products::CommandError::Repo(RepoError::Backend(
            "connection refused to db.internal:5432".to_string(),
        )));
        assert_eq!(e.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(e.code, "internal_error");
        assert_eq!(e.message, "internal error");
    }
}
