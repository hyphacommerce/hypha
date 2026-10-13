use axum::{
    Json,
    extract::rejection::{JsonRejection, PathRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use hypha_core::{ErrorCode, RepoError};

/// Every error the API returns, as `{"code": "<stable_code>", "error": "<message>"}`.
///
/// `code` comes from the domain's `ErrorCode` impls (shared with future MCP/CLI hosts)
/// and is what clients should match on; `error` is for humans and may change.
///
/// Status rules: 403 missing scope; 404 when the resource in the URL doesn't exist;
/// 409 conflicts with existing data or a concurrent change (`concurrent_modification`); 422 invalid input, including IDs in the body
/// that don't exist; 500 storage failures (message hidden, logged instead).
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl ApiError {
    fn new(status: StatusCode, code: &'static str, message: impl ToString) -> Self {
        Self {
            status,
            code,
            message: message.to_string(),
        }
    }

    pub fn route_not_found() -> Self {
        Self::new(StatusCode::NOT_FOUND, "route_not_found", "no such route")
    }

    pub fn method_not_allowed() -> Self {
        Self::new(
            StatusCode::METHOD_NOT_ALLOWED,
            "method_not_allowed",
            "method not allowed for this route",
        )
    }

    pub(crate) fn internal(error: impl std::fmt::Display) -> Self {
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
        let body = Json(serde_json::json!({ "code": self.code, "error": self.message }));
        (self.status, body).into_response()
    }
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

impl From<hypha_products::CommandError> for ApiError {
    fn from(e: hypha_products::CommandError) -> Self {
        use hypha_products::CommandError as E;
        let status = match &e {
            E::Auth(_) => StatusCode::FORBIDDEN,
            E::Product(_) | E::Money(_) => StatusCode::UNPROCESSABLE_ENTITY,
            E::HandleTaken(_) => StatusCode::CONFLICT,
            E::NotFound(_) => StatusCode::NOT_FOUND,
            E::Repo(RepoError::Backend(_)) => return Self::internal(e),
            E::Repo(RepoError::StaleVersion | RepoError::Duplicate(_)) => StatusCode::CONFLICT,
        };
        Self::domain(status, e)
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
            E::Repo(RepoError::StaleVersion | RepoError::Duplicate(_)) => StatusCode::CONFLICT,
        };
        Self::domain(status, e)
    }
}

impl From<hypha_cart::CommandError> for ApiError {
    fn from(e: hypha_cart::CommandError) -> Self {
        use hypha_cart::CommandError as E;
        let status = match &e {
            E::Auth(_) => StatusCode::FORBIDDEN,
            E::CartNotFound(_) => StatusCode::NOT_FOUND,
            E::Cart(_)
            | E::Line(_)
            | E::Money(_)
            | E::MarketNotFound(_)
            | E::NoMarketForCountry(_)
            | E::VariantNotFound(_) => StatusCode::UNPROCESSABLE_ENTITY,
            E::Repo(RepoError::Backend(_)) => return Self::internal(e),
            E::Repo(RepoError::StaleVersion | RepoError::Duplicate(_)) => StatusCode::CONFLICT,
        };
        Self::domain(status, e)
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
            E::TaxRegionExists { .. } | E::Tax(TaxError::DuplicateClassRate(_)) => {
                StatusCode::CONFLICT
            }
            E::Country(_) | E::Money(_) | E::TaxClassNotFound(_) => {
                StatusCode::UNPROCESSABLE_ENTITY
            }
            E::Repo(RepoError::Backend(_)) => return Self::internal(e),
            E::Repo(RepoError::StaleVersion | RepoError::Duplicate(_)) => StatusCode::CONFLICT,
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
            | E::Inventory(I::OnHandChanged { .. } | I::ReferenceInUse(_) | I::OutOfStock { .. }) => {
                StatusCode::CONFLICT
            }
            E::Inventory(
                I::EmptyLocationName
                | I::DuplicateMarket(_)
                | I::StockOutOfRange(_)
                | I::ZeroQuantity
                | I::InvalidReference,
            ) => StatusCode::UNPROCESSABLE_ENTITY,
            // Only ever produced when rebuilding stored data.
            E::Inventory(I::DuplicateLevel(_) | I::DuplicateReservation { .. }) => {
                return Self::internal(e);
            }
            E::Repo(RepoError::Backend(_)) => return Self::internal(e),
            E::Repo(RepoError::StaleVersion | RepoError::Duplicate(_)) => StatusCode::CONFLICT,
        };
        Self::domain(status, e)
    }
}

#[cfg(test)]
mod tests {
    use hypha_core::{AuthError, CartId, Scope};
    use uuid::Uuid;

    use super::*;

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
    fn storage_error_hides_message() {
        let e = ApiError::from(hypha_products::CommandError::Repo(RepoError::Backend(
            "connection refused to db.internal:5432".to_string(),
        )));
        assert_eq!(e.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(e.code, "internal_error");
        assert_eq!(e.message, "internal error");
    }
}
