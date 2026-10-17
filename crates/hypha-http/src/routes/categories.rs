//! Product categories (design note 0022): the tree, its membership, a category's image,
//! and what storefronts see of them.

use std::collections::HashMap;

use axum::{Json, extract::State, http::StatusCode};
use hypha_categories::{
    Category, CategoryTree, CategoryView, CreateCategory, UpdateCategory, add_category_products,
    create_category, delete_category, get_category, get_shown_category_by_handle, list_categories,
    list_category_products, remove_category_products, set_product_categories, shown_categories,
    update_category,
};
use hypha_core::{AssetId, CategoryId, ProductId};
use hypha_merchandising::{category_image, set_category_image};
use hypha_products::{HasProducts, get_product, get_products};
use serde::{Deserialize, Serialize};

use super::{
    page::{ListQuery, PageParams, PageResponse},
    products::{ProductMediaResponse, StatusDto, media_items},
    stamp_times,
};
use crate::{
    error::ApiError,
    extract::{Caller, JsonBody, PathId, Storefront},
    media::Media,
    patch,
    state::{SharedState, Storage},
};

/// A category in the admin's tree: what a list of categories shows, with its children.
#[derive(Serialize, utoipa::ToSchema)]
pub struct CategoryNode {
    id: CategoryId,
    name: String,
    handle: String,
    visible: bool,
    /// Among its siblings, from 0.
    position: u32,
    #[schema(no_recursion)]
    children: Vec<CategoryNode>,
}

fn nodes(tree: &CategoryTree, parent: Option<CategoryId>) -> Vec<CategoryNode> {
    tree.children(parent)
        .into_iter()
        .map(|c| CategoryNode {
            id: c.id(),
            name: c.name().to_string(),
            handle: c.handle().as_str().to_string(),
            visible: c.visible(),
            position: c.position(),
            children: nodes(tree, Some(c.id())),
        })
        .collect()
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct CategoryTreeResponse {
    /// The top-level categories, each with its children, in order. A store has tens or
    /// hundreds, so the whole tree comes in one response.
    items: Vec<CategoryNode>,
}

/// Every category, as a tree.
#[utoipa::path(
    get,
    path = "/admin/categories",
    responses((status = 200, description = "OK", body = CategoryTreeResponse)),
    extensions(("x-hypha-scopes" = json!(["products:read"]))),
)]
pub async fn list<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
) -> Result<Json<CategoryTreeResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let tree = list_categories(&ctx, &ports).await?;
    Ok(Json(CategoryTreeResponse {
        items: nodes(&tree, None),
    }))
}

/// A category next to it in the tree: an ancestor or a child.
#[derive(Serialize, utoipa::ToSchema)]
pub struct CategoryRef {
    id: CategoryId,
    name: String,
    handle: String,
}

impl From<&Category> for CategoryRef {
    fn from(c: &Category) -> Self {
        Self {
            id: c.id(),
            name: c.name().to_string(),
            handle: c.handle().as_str().to_string(),
        }
    }
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct CategoryResponse {
    id: CategoryId,
    name: String,
    handle: String,
    /// Sanitised HTML (scripts, styles and event attributes removed), safe to render.
    description: Option<String>,
    /// The image from the media library; `null` without one.
    image: Option<ProductMediaResponse>,
    parent: Option<CategoryId>,
    /// Among its siblings, from 0.
    position: u32,
    /// `false` hides it, and everything below it, from the storefront.
    visible: bool,
    /// Nearest first: its parent, its parent's parent, … (a breadcrumb, reversed).
    ancestors: Vec<CategoryRef>,
    /// In order.
    children: Vec<CategoryRef>,
    /// When it was created, RFC 3339 in UTC.
    created_at: String,
    /// When it last changed, RFC 3339 in UTC.
    updated_at: String,
}

impl CategoryResponse {
    async fn new(
        view: &CategoryView,
        image: Option<hypha_media::Asset>,
        media: Option<&Media>,
    ) -> Result<Self, ApiError> {
        let c = &view.category;
        let (created_at, updated_at) = stamp_times(hypha_core::Stamped::stamps(c))?;
        let image = match image {
            Some(asset) => media_items(std::slice::from_ref(&asset), media)
                .await?
                .pop(),
            None => None,
        };
        Ok(Self {
            id: c.id(),
            name: c.name().to_string(),
            handle: c.handle().as_str().to_string(),
            description: c.description().map(|d| d.as_html().to_string()),
            image,
            parent: c.parent(),
            position: c.position(),
            visible: c.visible(),
            ancestors: view.ancestors.iter().map(CategoryRef::from).collect(),
            children: view.children.iter().map(CategoryRef::from).collect(),
            created_at,
            updated_at,
        })
    }
}

/// The admin's response for category `id`, read back whole.
async fn respond<S: Storage>(
    state: &SharedState<S>,
    ports: &S::Ports<'_>,
    ctx: &hypha_core::Ctx,
    id: CategoryId,
) -> Result<CategoryResponse, ApiError> {
    let view = get_category(ctx, ports, id)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("category_not_found"))?;
    let image = category_image(ctx, ports, &view.category).await?;
    CategoryResponse::new(&view, image, state.media.as_ref()).await
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateCategoryRequest {
    /// 1 to 255 characters, trimmed.
    name: String,
    /// Lowercase letters, digits and single dashes; unique across categories.
    handle: String,
    /// HTML, sanitised when stored (at most 64 KiB).
    #[serde(default)]
    description: Option<String>,
    /// `null` or absent: a top-level category.
    #[serde(default)]
    parent: Option<CategoryId>,
    /// Among its siblings, from 0; absent or past the end: last.
    #[serde(default)]
    position: Option<u32>,
    /// Whether the storefront shows it. Required: there's no default (CA10).
    visible: bool,
}

/// Creates a category. Its siblings after `position` move one along.
#[utoipa::path(
    post,
    path = "/admin/categories",
    request_body = CreateCategoryRequest,
    responses((status = 201, description = "Created", body = CategoryResponse)),
    extensions(("x-hypha-scopes" = json!(["products:write"]))),
)]
pub async fn create<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    body: JsonBody<CreateCategoryRequest>,
) -> Result<(StatusCode, Json<CategoryResponse>), ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let input = CreateCategory {
        name: req.name,
        handle: req.handle,
        description: req.description,
        parent: req.parent,
        position: req.position,
        visible: req.visible,
    };
    let created =
        create_category(&ctx, &ports, &state.ids, &state.config.categories, input).await?;
    Ok((
        StatusCode::CREATED,
        Json(respond(&state, &ports, &ctx, created.id()).await?),
    ))
}

/// A category with its ancestors and children.
#[utoipa::path(
    get,
    path = "/admin/categories/{id}",
    params(("id" = CategoryId, Path)),
    responses((status = 200, description = "OK", body = CategoryResponse)),
    extensions(("x-hypha-scopes" = json!(["products:read"]))),
)]
pub async fn fetch<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<CategoryId>,
) -> Result<Json<CategoryResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    Ok(Json(respond(&state, &ports, &ctx, id).await?))
}

/// A JSON Merge Patch of a category: absent keeps, `null` clears (the description) or
/// moves to the top level (the parent).
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateCategoryRequest {
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = String, required = false)]
    name: Option<String>,
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = String, required = false)]
    handle: Option<String>,
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<String>, required = false)]
    description: Option<Option<String>>,
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = bool, required = false)]
    visible: Option<bool>,
    /// Moves it under another category, or to the top level with `null`. Refused when it
    /// would sit under itself or a descendant, or nest deeper than allowed.
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<CategoryId>, required = false)]
    parent: Option<Option<CategoryId>>,
    /// Among its (new) siblings, from 0; moving without one puts it last.
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = u32, required = false)]
    position: Option<u32>,
}

/// Renames, describes, shows or hides, moves or reorders a category.
#[utoipa::path(
    patch,
    path = "/admin/categories/{id}",
    params(("id" = CategoryId, Path)),
    request_body = UpdateCategoryRequest,
    responses((status = 200, description = "OK", body = CategoryResponse)),
    extensions(("x-hypha-scopes" = json!(["products:write"]))),
)]
pub async fn update<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<CategoryId>,
    body: JsonBody<UpdateCategoryRequest>,
) -> Result<Json<CategoryResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let input = UpdateCategory {
        name: req.name,
        handle: req.handle,
        description: req.description,
        visible: req.visible,
        parent: req.parent,
        position: req.position,
    };
    let in_url = |e: hypha_categories::CommandError| match e {
        hypha_categories::CommandError::CategoryNotFound(c) if c == id => {
            ApiError::from(e).not_found_if("category_not_found")
        }
        e => e.into(),
    };
    update_category(&ctx, &ports, &state.config.categories, id, input)
        .await
        .map_err(in_url)?;
    Ok(Json(respond(&state, &ports, &ctx, id).await?))
}

/// Deletes a category without subcategories, with its membership; its products stay.
#[utoipa::path(
    delete,
    path = "/admin/categories/{id}",
    params(("id" = CategoryId, Path)),
    responses((status = 204, description = "No content")),
    extensions(("x-hypha-scopes" = json!(["products:write"]))),
)]
pub async fn delete<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<CategoryId>,
) -> Result<StatusCode, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    delete_category(&ctx, &ports, id)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("category_not_found"))?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CategoryProductsRequest {
    /// Product IDs aren't checked: one that names no product is stored and never shows.
    products: Vec<ProductId>,
}

/// Adds products to a category; ones already in it are ignored.
#[utoipa::path(
    post,
    path = "/admin/categories/{id}/products",
    params(("id" = CategoryId, Path)),
    request_body = CategoryProductsRequest,
    responses((status = 204, description = "No content")),
    extensions(("x-hypha-scopes" = json!(["products:write"]))),
)]
pub async fn add_products<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<CategoryId>,
    body: JsonBody<CategoryProductsRequest>,
) -> Result<StatusCode, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    add_category_products(&ctx, &ports, id, req.products)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("category_not_found"))?;
    Ok(StatusCode::NO_CONTENT)
}

/// Removes products from a category; ones not in it are ignored.
#[utoipa::path(
    delete,
    path = "/admin/categories/{id}/products",
    params(("id" = CategoryId, Path)),
    request_body = CategoryProductsRequest,
    responses((status = 204, description = "No content")),
    extensions(("x-hypha-scopes" = json!(["products:write"]))),
)]
pub async fn remove_products<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<CategoryId>,
    body: JsonBody<CategoryProductsRequest>,
) -> Result<StatusCode, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    remove_category_products(&ctx, &ports, id, req.products)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("category_not_found"))?;
    Ok(StatusCode::NO_CONTENT)
}

/// A product directly in a category. Product IDs are stored as given, so the product's
/// fields are `null` for one that names no product.
#[derive(Serialize, utoipa::ToSchema)]
pub struct CategoryProductResponse {
    id: ProductId,
    title: Option<String>,
    handle: Option<String>,
    status: Option<StatusDto>,
}

type CategoryProductPage = PageResponse<CategoryProductResponse>;

/// The products directly in a category, newest first (a storefront lists a category
/// with its subcategories through search).
#[utoipa::path(
    get,
    path = "/admin/categories/{id}/products",
    params(("id" = CategoryId, Path), PageParams),
    responses((status = 200, description = "OK", body = CategoryProductPage)),
    extensions(("x-hypha-scopes" = json!(["products:read"]))),
)]
pub async fn list_products<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<CategoryId>,
    query: ListQuery,
) -> Result<Json<CategoryProductPage>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let (page, _, scope) = query.parse()?;
    let page = list_category_products(&ctx, &ports, id, page)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("category_not_found"))?;
    let mut products: HashMap<ProductId, _> = get_products(&ctx, ports.products(), &page.items)
        .await?
        .into_iter()
        .map(|p| (p.id(), p))
        .collect();
    Ok(Json(PageResponse::new(
        page,
        &scope,
        |id| *id,
        |id| {
            let product = products.remove(&id);
            Ok(CategoryProductResponse {
                id,
                title: product.as_ref().map(|p| p.title().to_string()),
                handle: product.as_ref().map(|p| p.handle().as_str().to_string()),
                status: product.as_ref().map(|p| p.status().into()),
            })
        },
    )?))
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CategoryImageRequest {
    /// An image asset that's uploaded and public; `null` clears it.
    image: Option<AssetId>,
}

/// Sets or clears a category's image. The asset is in use while it's set, so it can't
/// be deleted from the media library.
#[utoipa::path(
    put,
    path = "/admin/categories/{id}/image",
    params(("id" = CategoryId, Path)),
    request_body = CategoryImageRequest,
    responses((status = 200, description = "OK", body = CategoryResponse)),
    extensions(("x-hypha-scopes" = json!(["products:write"]))),
)]
pub async fn set_image<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<CategoryId>,
    body: JsonBody<CategoryImageRequest>,
) -> Result<Json<CategoryResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    set_category_image(&ctx, &ports, id, req.image).await?;
    Ok(Json(respond(&state, &ports, &ctx, id).await?))
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ProductCategoriesRequest {
    /// Every category the product should be in; it leaves the others.
    categories: Vec<CategoryId>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct ProductCategoriesResponse {
    /// Sorted.
    categories: Vec<CategoryId>,
}

/// Sets the categories a product is in (the product page's Categories card).
#[utoipa::path(
    put,
    path = "/admin/products/{id}/categories",
    params(("id" = ProductId, Path)),
    request_body = ProductCategoriesRequest,
    responses((status = 200, description = "OK", body = ProductCategoriesResponse)),
    extensions(("x-hypha-scopes" = json!(["products:write"]))),
)]
pub async fn set_for_product<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<ProductId>,
    body: JsonBody<ProductCategoriesRequest>,
) -> Result<Json<ProductCategoriesResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    // The product is in the URL: a 404 when it doesn't exist.
    get_product(&ctx, ports.products(), id).await?;
    let categories = set_product_categories(&ctx, &ports, id, req.categories).await?;
    Ok(Json(ProductCategoriesResponse { categories }))
}

// ---------- storefront ----------

/// A category in the storefront's tree, with its shown children.
#[derive(Serialize, utoipa::ToSchema)]
pub struct StorefrontCategoryNode {
    id: CategoryId,
    name: String,
    handle: String,
    #[schema(no_recursion)]
    children: Vec<StorefrontCategoryNode>,
}

fn storefront_nodes(
    tree: &CategoryTree,
    parent: Option<CategoryId>,
) -> Vec<StorefrontCategoryNode> {
    tree.children(parent)
        .into_iter()
        .map(|c| StorefrontCategoryNode {
            id: c.id(),
            name: c.name().to_string(),
            handle: c.handle().as_str().to_string(),
            children: storefront_nodes(tree, Some(c.id())),
        })
        .collect()
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct StorefrontCategoryTreeResponse {
    /// The shown top-level categories, each with its shown children, in order. A hidden
    /// category is left out with everything under it.
    items: Vec<StorefrontCategoryNode>,
}

/// The categories a storefront shows, as a tree (navigation menus).
#[utoipa::path(
    get,
    path = "/storefront/categories",
    responses((status = 200, description = "OK", body = StorefrontCategoryTreeResponse)),
    extensions(("x-hypha-scopes" = json!(["products:read"]))),
)]
pub async fn list_storefront<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller<Storefront>,
) -> Result<Json<StorefrontCategoryTreeResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let tree = shown_categories(&ctx, &ports).await?;
    Ok(Json(StorefrontCategoryTreeResponse {
        items: storefront_nodes(&tree, None),
    }))
}

/// A category page's category: no admin fields, only shown children.
#[derive(Serialize, utoipa::ToSchema)]
pub struct StorefrontCategoryResponse {
    id: CategoryId,
    name: String,
    handle: String,
    /// Sanitised HTML, safe to render.
    description: Option<String>,
    image: Option<ProductMediaResponse>,
    /// Nearest first: a breadcrumb, reversed.
    ancestors: Vec<CategoryRef>,
    /// Its shown children, in order.
    children: Vec<CategoryRef>,
}

/// A category page (design note 0022, section 4): the category by handle, with its
/// ancestors and shown children. Its products come through
/// `/storefront/search?category=…`. A hidden category (or one under a hidden one) is
/// `404 category_not_found`.
#[utoipa::path(
    get,
    path = "/storefront/categories/by-handle/{handle}",
    params(("handle" = String, Path, description = "The category's handle.", example = "shirts")),
    responses((status = 200, description = "OK", body = StorefrontCategoryResponse)),
    extensions(("x-hypha-scopes" = json!(["products:read"]))),
)]
pub async fn fetch_by_handle_storefront<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller<Storefront>,
    PathId(handle): PathId<String>,
) -> Result<Json<StorefrontCategoryResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let view = get_shown_category_by_handle(&ctx, &ports, &handle).await?;
    let image = match category_image(&ctx, &ports, &view.category).await? {
        Some(asset) => media_items(std::slice::from_ref(&asset), state.media.as_ref())
            .await?
            .pop(),
        None => None,
    };
    let c = &view.category;
    Ok(Json(StorefrontCategoryResponse {
        id: c.id(),
        name: c.name().to_string(),
        handle: c.handle().as_str().to_string(),
        description: c.description().map(|d| d.as_html().to_string()),
        image,
        ancestors: view.ancestors.iter().map(CategoryRef::from).collect(),
        children: view.children.iter().map(CategoryRef::from).collect(),
    }))
}
