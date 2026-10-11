use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use hypha_core::{Ctx, IdGenerator, Money, ProductId, VariantId};
use hypha_products::{
    CommandError, CreateSimpleProduct, Product, ProductStatus, create_simple_product, get_product,
};
use hypha_testkit::InMemoryProductRepo;
use serde::{Deserialize, Serialize};
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

// ---------- ports implemented by this host ----------

/// Real IDs: UUIDv7 (time-ordered), generated from the system clock and RNG.
struct UuidV7Ids;

impl IdGenerator for UuidV7Ids {
    fn new_uuid(&self) -> Uuid {
        Uuid::now_v7()
    }
}

// ---------- shared state ----------

struct AppState {
    products: InMemoryProductRepo,
    ids: UuidV7Ids,
}

type SharedState = Arc<AppState>;

// ---------- request / response shapes (the HTTP API's own types) ----------

#[derive(Deserialize)]
struct CreateProductRequest {
    handle: String,
    title: String,
    price: Money,
}

#[derive(Serialize)]
struct ProductResponse {
    id: ProductId,
    handle: String,
    title: String,
    status: ProductStatus,
    variants: Vec<VariantResponse>,
}

#[derive(Serialize)]
struct VariantResponse {
    id: VariantId,
    price: Money,
}

impl From<&Product> for ProductResponse {
    fn from(p: &Product) -> Self {
        Self {
            id: p.id(),
            handle: p.handle().as_str().to_string(),
            title: p.title().to_string(),
            status: p.status(),
            variants: p
                .variants()
                .iter()
                .map(|v| VariantResponse {
                    id: v.id(),
                    price: v.base_price().money(),
                })
                .collect(),
        }
    }
}

// ---------- errors -> HTTP ----------

/// Wrapper so we can implement axum's IntoResponse for the core's CommandError.
struct ApiError(CommandError);

impl From<CommandError> for ApiError {
    fn from(e: CommandError) -> Self {
        Self(e)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match &self.0 {
            CommandError::Auth(_) => StatusCode::FORBIDDEN,
            CommandError::Product(_) | CommandError::Money(_) => StatusCode::UNPROCESSABLE_ENTITY,
            CommandError::HandleTaken(_) => StatusCode::CONFLICT,
            CommandError::NotFound(_) => StatusCode::NOT_FOUND,
            CommandError::Repo(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let body = Json(serde_json::json!({ "error": self.0.to_string() }));
        (status, body).into_response()
    }
}

// ---------- handlers ----------

async fn create_product(
    State(state): State<SharedState>,
    Json(req): Json<CreateProductRequest>,
) -> Result<(StatusCode, Json<ProductResponse>), ApiError> {
    let ctx = Ctx::system(); // TODO: real auth (API keys / tokens)
    let input = CreateSimpleProduct {
        handle: req.handle,
        title: req.title,
        price: req.price,
    };
    let product = create_simple_product(&ctx, &state.products, &state.ids, input).await?;
    Ok((StatusCode::CREATED, Json(ProductResponse::from(&product))))
}

async fn fetch_product(
    State(state): State<SharedState>,
    Path(id): Path<ProductId>,
) -> Result<Json<ProductResponse>, ApiError> {
    let ctx = Ctx::system(); // TODO: real auth
    let product = get_product(&ctx, &state.products, id).await?;
    Ok(Json(ProductResponse::from(&product)))
}

// ---------- startup ----------

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,tower_http=debug".into()),
        )
        .init();

    let state: SharedState = Arc::new(AppState {
        products: InMemoryProductRepo::default(),
        ids: UuidV7Ids,
    });

    let app = Router::new()
        .route("/products", post(create_product))
        .route("/products/{id}", get(fetch_product))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .expect("port 3000 should be free");
    tracing::info!("Hypha listening on http://127.0.0.1:3000");
    axum::serve(listener, app).await.expect("server error");
}
