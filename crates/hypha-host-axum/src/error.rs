use axum::{
    Json,
    extract::rejection::{JsonRejection, PathRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
};

/// Every error the API returns, as `{"error": "<message>"}` with a matching status.
///
/// Status rules: 403 missing scope; 404 when the resource in the URL doesn't exist;
/// 409 conflicts with existing data; 422 invalid input, including IDs in the body
/// that don't exist; 500 storage failures (message hidden, logged instead).
pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn new(status: StatusCode, message: impl ToString) -> Self {
        Self {
            status,
            message: message.to_string(),
        }
    }

    pub fn not_found(message: impl ToString) -> Self {
        Self::new(StatusCode::NOT_FOUND, message)
    }

    pub fn method_not_allowed() -> Self {
        Self::new(
            StatusCode::METHOD_NOT_ALLOWED,
            "method not allowed for this route",
        )
    }

    fn internal(error: impl std::fmt::Display) -> Self {
        tracing::error!(%error, "internal error");
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = Json(serde_json::json!({ "error": self.message }));
        (self.status, body).into_response()
    }
}

// ---------- axum's own rejections (bad JSON, bad path IDs) ----------

impl From<JsonRejection> for ApiError {
    fn from(rejection: JsonRejection) -> Self {
        Self::new(rejection.status(), rejection.body_text())
    }
}

impl From<PathRejection> for ApiError {
    fn from(rejection: PathRejection) -> Self {
        Self::new(rejection.status(), rejection.body_text())
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
            E::Repo(_) => return Self::internal(e),
        };
        Self::new(status, e)
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
            | E::CurrencyMismatch { .. }
            | E::DuplicateFixedPrice(_) => StatusCode::UNPROCESSABLE_ENTITY,
            E::Repo(_) => return Self::internal(e),
        };
        Self::new(status, e)
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
            | E::VariantNotFound(_) => StatusCode::UNPROCESSABLE_ENTITY,
            E::Repo(_) => return Self::internal(e),
        };
        Self::new(status, e)
    }
}
