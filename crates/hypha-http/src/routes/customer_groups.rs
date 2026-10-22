//! Customer groups: named, static sets of customers with a stable ID. Membership is written
//! as sets here and read through the customers list (`GET /admin/customers?group=`) and a
//! customer's `groups`.

use axum::{Json, extract::State, http::StatusCode};
use hypha_core::{CustomerGroupId, CustomerId, Stamped};
use hypha_customers::{
    CreateCustomerGroup, CustomerGroup, UpdateCustomerGroup, add_group_members, create_group,
    delete_group, get_group, list_groups, remove_group_members, update_group,
};
use serde::{Deserialize, Serialize};

use super::stamp_times;
use crate::{
    error::ApiError,
    extract::{Caller, JsonBody, PathId},
    patch,
    state::{SharedState, Storage},
};

#[derive(Serialize, utoipa::ToSchema)]
pub struct CustomerGroupResponse {
    id: CustomerGroupId,
    name: String,
    /// Lowercase letters, digits and single dashes; unique among groups.
    handle: String,
    description: Option<String>,
    /// RFC 3339 in UTC.
    created_at: String,
    updated_at: String,
}

impl CustomerGroupResponse {
    fn new(g: &CustomerGroup) -> Result<Self, ApiError> {
        let (created_at, updated_at) = stamp_times(g.stamps())?;
        Ok(Self {
            id: g.id(),
            name: g.name().to_string(),
            handle: g.handle().as_str().to_string(),
            description: g.description().map(str::to_string),
            created_at,
            updated_at,
        })
    }
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct CustomerGroupList {
    /// By name, ignoring case. A store has few groups, so they come in one response.
    items: Vec<CustomerGroupResponse>,
}

/// Every customer group, by name.
#[utoipa::path(
    get,
    path = "/admin/customer-groups",
    responses((status = 200, description = "OK", body = CustomerGroupList)),
    extensions(("x-hypha-scopes" = json!(["customers:read"]))),
)]
pub async fn list<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
) -> Result<Json<CustomerGroupList>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let groups = list_groups(&ctx, &ports).await?;
    Ok(Json(CustomerGroupList {
        items: groups
            .iter()
            .map(CustomerGroupResponse::new)
            .collect::<Result<_, _>>()?,
    }))
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateCustomerGroupRequest {
    /// 1 to 255 characters, trimmed; unique among groups ignoring case.
    name: String,
    /// Lowercase letters, digits and single dashes; unique among groups.
    handle: String,
    /// Plain text, up to 1,000 characters.
    #[serde(default)]
    description: Option<String>,
}

/// Creates a customer group.
#[utoipa::path(
    post,
    path = "/admin/customer-groups",
    request_body = CreateCustomerGroupRequest,
    responses((status = 201, description = "Created", body = CustomerGroupResponse)),
    extensions(("x-hypha-scopes" = json!(["customers:write"]))),
)]
pub async fn create<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    body: JsonBody<CreateCustomerGroupRequest>,
) -> Result<(StatusCode, Json<CustomerGroupResponse>), ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let input = CreateCustomerGroup {
        name: req.name,
        handle: req.handle,
        description: req.description,
    };
    let created = create_group(&ctx, &ports, &state.ids, input).await?;
    Ok((
        StatusCode::CREATED,
        Json(CustomerGroupResponse::new(&created)?),
    ))
}

/// A customer group. Its customers: `GET /admin/customers?group={id}`.
#[utoipa::path(
    get,
    path = "/admin/customer-groups/{id}",
    params(("id" = CustomerGroupId, Path)),
    responses((status = 200, description = "OK", body = CustomerGroupResponse)),
    extensions(("x-hypha-scopes" = json!(["customers:read"]))),
)]
pub async fn fetch<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<CustomerGroupId>,
) -> Result<Json<CustomerGroupResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let group = get_group(&ctx, &ports, id)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("customer_group_not_found"))?;
    Ok(Json(CustomerGroupResponse::new(&group)?))
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateCustomerGroupRequest {
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = String, required = false)]
    name: Option<String>,
    #[serde(default, deserialize_with = "patch::non_null")]
    #[schema(value_type = String, required = false)]
    handle: Option<String>,
    /// `null` clears it.
    #[serde(default, deserialize_with = "patch::nullable")]
    #[schema(value_type = Option<String>, required = false)]
    description: Option<Option<String>>,
}

/// Renames, re-handles or describes a group (JSON Merge Patch).
#[utoipa::path(
    patch,
    path = "/admin/customer-groups/{id}",
    params(("id" = CustomerGroupId, Path)),
    request_body = UpdateCustomerGroupRequest,
    responses((status = 200, description = "OK", body = CustomerGroupResponse)),
    extensions(("x-hypha-scopes" = json!(["customers:write"]))),
)]
pub async fn update<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<CustomerGroupId>,
    body: JsonBody<UpdateCustomerGroupRequest>,
) -> Result<Json<CustomerGroupResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let input = UpdateCustomerGroup {
        name: req.name,
        handle: req.handle,
        description: req.description,
    };
    let group = update_group(&ctx, &ports, id, input)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("customer_group_not_found"))?;
    Ok(Json(CustomerGroupResponse::new(&group)?))
}

/// Deletes a group with its membership; its customers stay. Once prices or payment
/// methods point at groups, one in use is refused.
#[utoipa::path(
    delete,
    path = "/admin/customer-groups/{id}",
    params(("id" = CustomerGroupId, Path)),
    responses((status = 204, description = "No content")),
    extensions(("x-hypha-scopes" = json!(["customers:write"]))),
)]
pub async fn remove<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<CustomerGroupId>,
) -> Result<StatusCode, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    delete_group(&ctx, &ports, id)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("customer_group_not_found"))?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct GroupCustomersRequest {
    /// Each must exist (`422 customer_not_found`).
    customers: Vec<CustomerId>,
}

/// Adds customers to a group; ones already in it are ignored. One
/// `customer_group.members_added` for what changed.
#[utoipa::path(
    post,
    path = "/admin/customer-groups/{id}/customers",
    params(("id" = CustomerGroupId, Path)),
    request_body = GroupCustomersRequest,
    responses((status = 204, description = "No content")),
    extensions(("x-hypha-scopes" = json!(["customers:write"]))),
)]
pub async fn add_customers<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<CustomerGroupId>,
    body: JsonBody<GroupCustomersRequest>,
) -> Result<StatusCode, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    // The group is the URL's; the customers come in the body.
    add_group_members(&ctx, &ports, id, req.customers)
        .await
        .map_err(|e| {
            ApiError::from(e)
                .not_found_if("customer_group_not_found")
                .unprocessable_if("customer_not_found")
        })?;
    Ok(StatusCode::NO_CONTENT)
}

/// Removes customers from a group; ones not in it are ignored.
#[utoipa::path(
    delete,
    path = "/admin/customer-groups/{id}/customers",
    params(("id" = CustomerGroupId, Path)),
    request_body = GroupCustomersRequest,
    responses((status = 204, description = "No content")),
    extensions(("x-hypha-scopes" = json!(["customers:write"]))),
)]
pub async fn remove_customers<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<CustomerGroupId>,
    body: JsonBody<GroupCustomersRequest>,
) -> Result<StatusCode, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    remove_group_members(&ctx, &ports, id, req.customers)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("customer_group_not_found"))?;
    Ok(StatusCode::NO_CONTENT)
}
