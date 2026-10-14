//! The logged-in staff member's own account: who am I, password, two-factor setup
//! (design note 0008, section 6). Each route needs a session, not an API key.

use axum::{Json, extract::State, http::StatusCode};
use hypha_auth::{
    HasIdentities, IdentityRepo, change_password, confirm_totp, disable_mfa,
    regenerate_recovery_codes, start_totp,
};
use hypha_core::{RoleId, Scope};
use hypha_stores::get_store;
use serde::{Deserialize, Serialize};

use super::staff::UserResponse;
use crate::{
    error::ApiError,
    extract::{JsonBody, StaffCaller},
    state::{SharedState, Storage},
};

#[derive(Serialize)]
pub(super) struct MeResponse {
    user: UserResponse,
    role: MeRole,
    /// What the session may do: the role's scopes, spelled out for the owner.
    scopes: Vec<Scope>,
    mfa_enabled: bool,
}

#[derive(Serialize)]
pub(super) struct MeRole {
    id: RoleId,
    name: String,
}

pub(super) async fn fetch<S: Storage>(
    State(state): State<SharedState<S>>,
    StaffCaller(staff): StaffCaller,
) -> Result<Json<MeResponse>, ApiError> {
    let ports = state.ports().await?;
    let identity = ports
        .identities()
        .get(staff.session.identity())
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::internal("session without identity"))?;
    Ok(Json(MeResponse {
        user: UserResponse::new(&staff.user)?,
        role: MeRole {
            id: staff.role.id(),
            name: staff.role.name().to_string(),
        },
        scopes: staff.role.granted(),
        mfa_enabled: identity.mfa_enabled(),
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ChangePasswordRequest {
    current_password: String,
    new_password: String,
}

/// Changes the password and logs out every other session.
pub(super) async fn change_password_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    StaffCaller(staff): StaffCaller,
    JsonBody(req): JsonBody<ChangePasswordRequest>,
) -> Result<StatusCode, ApiError> {
    let ports = state.ports().await?;
    change_password(
        &ports,
        &state.env,
        &staff.session,
        &req.current_password,
        &req.new_password,
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
pub(super) struct TotpSetupResponse {
    /// Base32, for typing into the app.
    secret: String,
    /// `otpauth://…`, for a QR code.
    uri: String,
}

/// Starts the authenticator setup; confirm with a code to turn 2FA on.
pub(super) async fn start_totp_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    StaffCaller(staff): StaffCaller,
) -> Result<(StatusCode, Json<TotpSetupResponse>), ApiError> {
    let ports = state.ports().await?;
    // The store's name labels the entry in the authenticator app. Read as the system:
    // labelling your own 2FA entry shouldn't need `stores:read`.
    let directory = state.storage.directory().await?;
    let store = get_store(&hypha_core::Ctx::system(), &directory, state.store).await?;
    let setup = start_totp(
        ports.identities(),
        &state.env,
        staff.session.identity(),
        store.name(),
        staff.user.email().as_str(),
    )
    .await?;
    Ok((
        StatusCode::CREATED,
        Json(TotpSetupResponse {
            secret: setup.secret,
            uri: setup.uri,
        }),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CodeRequest {
    /// From the authenticator app, or (to disable or regenerate) a recovery code.
    code: String,
}

#[derive(Serialize)]
pub(super) struct RecoveryCodesResponse {
    /// Shown only now: each works once if the authenticator is lost.
    recovery_codes: Vec<String>,
}

/// A wrong code here comes from someone logged in, so it's 422, not the 401 a login
/// gets (a 401 would tell the client its session died).
fn code_error(e: hypha_auth::CommandError) -> ApiError {
    ApiError::from(e).unprocessable_if("invalid_mfa_code")
}

pub(super) async fn confirm_totp_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    StaffCaller(staff): StaffCaller,
    JsonBody(req): JsonBody<CodeRequest>,
) -> Result<Json<RecoveryCodesResponse>, ApiError> {
    let ports = state.ports().await?;
    let codes = confirm_totp(
        ports.identities(),
        &state.env,
        staff.session.identity(),
        &req.code,
    )
    .await
    .map_err(code_error)?;
    Ok(Json(RecoveryCodesResponse {
        recovery_codes: codes,
    }))
}

pub(super) async fn disable_totp_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    StaffCaller(staff): StaffCaller,
    JsonBody(req): JsonBody<CodeRequest>,
) -> Result<StatusCode, ApiError> {
    let ports = state.ports().await?;
    disable_mfa(
        ports.identities(),
        &state.env,
        staff.session.identity(),
        &req.code,
    )
    .await
    .map_err(code_error)?;
    Ok(StatusCode::NO_CONTENT)
}

pub(super) async fn recovery_codes_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    StaffCaller(staff): StaffCaller,
    JsonBody(req): JsonBody<CodeRequest>,
) -> Result<Json<RecoveryCodesResponse>, ApiError> {
    let ports = state.ports().await?;
    let codes = regenerate_recovery_codes(
        ports.identities(),
        &state.env,
        staff.session.identity(),
        &req.code,
    )
    .await
    .map_err(code_error)?;
    Ok(Json(RecoveryCodesResponse {
        recovery_codes: codes,
    }))
}
