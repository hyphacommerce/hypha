//! Roles, staff users and invites (design note 0008, section 4).

use axum::{Json, extract::State, http::StatusCode};
use hypha_core::{Email, InviteId, RoleId, Scope, UserId};
use hypha_users::{
    HasRoles, HasUsers, Invite, InviteUser, Role, RoleScopes, UpdateRole, UpdateUser, User,
    UserStatus, create_role, get_role, get_user, invite_user, update_role, update_user,
};
use serde::{Deserialize, Serialize};

use super::rfc3339;
use crate::{
    error::ApiError,
    extract::{Caller, JsonBody, PathId},
    patch,
    state::{SharedState, Storage},
};

// ---------- roles ----------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateRoleRequest {
    name: String,
    scopes: Vec<Scope>,
}

/// JSON Merge Patch: only the fields sent change.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdateRoleRequest {
    #[serde(default, deserialize_with = "patch::non_null")]
    name: Option<String>,
    #[serde(default, deserialize_with = "patch::non_null")]
    scopes: Option<Vec<Scope>>,
}

#[derive(Serialize)]
pub(super) struct RoleResponse {
    id: RoleId,
    name: String,
    /// The owner role has every scope, including future ones.
    owner: bool,
    scopes: Vec<Scope>,
}

impl From<&Role> for RoleResponse {
    fn from(r: &Role) -> Self {
        Self {
            id: r.id(),
            name: r.name().to_string(),
            owner: matches!(r.scopes(), RoleScopes::All),
            scopes: r.granted(),
        }
    }
}

pub(super) async fn create_role_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    Caller(ctx): Caller,
    JsonBody(req): JsonBody<CreateRoleRequest>,
) -> Result<(StatusCode, Json<RoleResponse>), ApiError> {
    let ports = state.ports().await?;
    let role = create_role(&ctx, ports.roles(), &state.ids, &req.name, &req.scopes).await?;
    Ok((StatusCode::CREATED, Json(RoleResponse::from(&role))))
}

pub(super) async fn fetch_role<S: Storage>(
    State(state): State<SharedState<S>>,
    Caller(ctx): Caller,
    PathId(id): PathId<RoleId>,
) -> Result<Json<RoleResponse>, ApiError> {
    let ports = state.ports().await?;
    let role = get_role(&ctx, ports.roles(), id)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("role_not_found"))?;
    Ok(Json(RoleResponse::from(&role)))
}

pub(super) async fn update_role_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    Caller(ctx): Caller,
    PathId(id): PathId<RoleId>,
    JsonBody(req): JsonBody<UpdateRoleRequest>,
) -> Result<Json<RoleResponse>, ApiError> {
    let ports = state.ports().await?;
    let input = UpdateRole {
        name: req.name,
        scopes: req.scopes,
    };
    let role = update_role(&ctx, ports.roles(), id, input)
        .await
        .map_err(|e| ApiError::from(e).not_found_if("role_not_found"))?;
    Ok(Json(RoleResponse::from(&role)))
}

// ---------- users ----------

#[derive(Serialize)]
pub(super) struct UserResponse {
    id: UserId,
    email: Email,
    name: String,
    role: RoleId,
    status: &'static str,
    created_at: String,
}

impl UserResponse {
    pub(super) fn new(u: &User) -> Result<Self, ApiError> {
        Ok(Self {
            id: u.id(),
            email: u.email().clone(),
            name: u.name().to_string(),
            role: u.role(),
            status: u.status().as_str(),
            created_at: rfc3339(u.created_at())?,
        })
    }
}

/// JSON Merge Patch: only the fields sent change.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdateUserRequest {
    #[serde(default, deserialize_with = "patch::non_null")]
    name: Option<String>,
    #[serde(default, deserialize_with = "patch::non_null")]
    role: Option<RoleId>,
    #[serde(default, deserialize_with = "patch::non_null")]
    status: Option<StatusRequest>,
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub(super) enum StatusRequest {
    Active,
    Disabled,
}

pub(super) async fn fetch_user<S: Storage>(
    State(state): State<SharedState<S>>,
    Caller(ctx): Caller,
    PathId(id): PathId<UserId>,
) -> Result<Json<UserResponse>, ApiError> {
    let ports = state.ports().await?;
    let user = get_user(&ctx, ports.users(), id).await?;
    Ok(Json(UserResponse::new(&user)?))
}

/// A disabled user's sessions stop working on their next request.
pub(super) async fn update_user_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    Caller(ctx): Caller,
    PathId(id): PathId<UserId>,
    JsonBody(req): JsonBody<UpdateUserRequest>,
) -> Result<Json<UserResponse>, ApiError> {
    let ports = state.ports().await?;
    let input = UpdateUser {
        name: req.name,
        role: req.role,
        status: req.status.map(|s| match s {
            StatusRequest::Active => UserStatus::Active,
            StatusRequest::Disabled => UserStatus::Disabled,
        }),
    };
    let user = update_user(&ctx, &ports, id, input).await?;
    Ok(Json(UserResponse::new(&user)?))
}

// ---------- invites ----------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct InviteRequest {
    email: String,
    role: RoleId,
}

#[derive(Serialize)]
pub(super) struct InviteResponse {
    id: InviteId,
    email: Email,
    role: RoleId,
    expires_at: String,
}

impl InviteResponse {
    fn new(i: &Invite) -> Result<Self, ApiError> {
        Ok(Self {
            id: i.id(),
            email: i.email().clone(),
            role: i.role(),
            expires_at: rfc3339(i.expires_at())?,
        })
    }
}

/// Emails an invite (a new one replaces a pending one). The token is only in the email.
pub(super) async fn invite_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    Caller(ctx): Caller,
    JsonBody(req): JsonBody<InviteRequest>,
) -> Result<(StatusCode, Json<InviteResponse>), ApiError> {
    let ports = state.ports().await?;
    let input = InviteUser {
        email: req.email,
        role: req.role,
    };
    let invite = invite_user(&ctx, &ports, &state.env, input).await?;
    Ok((StatusCode::CREATED, Json(InviteResponse::new(&invite)?)))
}
