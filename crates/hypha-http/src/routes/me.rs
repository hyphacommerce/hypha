//! The logged-in staff member's own account: who am I, password, two-factor setup
//! (design note 0008, section 6). Each route needs a session, not an API key.

use axum::{Json, extract::State, http::StatusCode};
use hypha_auth::{
    HasIdentities, IdentityRepo, change_password, confirm_totp, disable_mfa,
    regenerate_recovery_codes, start_totp,
};
use hypha_core::{RoleId, Scope};
use serde::{Deserialize, Serialize};

use super::staff::UserResponse;
use crate::{
    error::ApiError,
    extract::{JsonBody, StaffCaller},
    state::{SharedState, Storage},
};

#[derive(Serialize, utoipa::ToSchema)]
pub struct MeResponse {
    user: UserResponse,
    role: MeRole,
    /// What the session may do: the role's scopes, spelled out for the owner.
    scopes: Vec<Scope>,
    mfa_enabled: bool,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct MeRole {
    id: RoleId,
    name: String,
}

/// The logged-in staff member: user, role, scopes and whether 2FA is on.
#[utoipa::path(
    get,
    path = "/admin/me",
    responses((status = 200, description = "OK", body = MeResponse)),
    extensions(("x-hypha-staff-session" = json!(true))),
)]
pub async fn fetch<S: Storage>(
    State(state): State<SharedState<S>>,
    staff: StaffCaller,
) -> Result<Json<MeResponse>, ApiError> {
    let (ports, staff) = staff.authenticate(&state).await?;
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

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ChangePasswordRequest {
    current_password: String,
    new_password: String,
}

/// Changes the password and logs out every other session.
#[utoipa::path(
    put,
    path = "/admin/me/password",
    request_body = ChangePasswordRequest,
    responses((status = 204, description = "No content")),
    extensions(("x-hypha-staff-session" = json!(true))),
)]
pub async fn change_password_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    staff: StaffCaller,
    body: JsonBody<ChangePasswordRequest>,
) -> Result<StatusCode, ApiError> {
    let (ports, staff) = staff.authenticate(&state).await?;
    let req = body.parse()?;
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

#[derive(Serialize, utoipa::ToSchema)]
pub struct TotpSetupResponse {
    /// Base32, for typing into the app.
    secret: String,
    /// `otpauth://…`, for a QR code.
    uri: String,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct StartTotpRequest {
    /// The current password: a stolen session alone can't put an authenticator on the
    /// account.
    password: String,
}

/// Starts the authenticator setup; confirm with a code to turn 2FA on.
#[utoipa::path(
    post,
    path = "/admin/me/mfa/totp",
    request_body = StartTotpRequest,
    responses((status = 201, description = "Created", body = TotpSetupResponse)),
    extensions(("x-hypha-staff-session" = json!(true))),
)]
pub async fn start_totp_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    staff: StaffCaller,
    body: JsonBody<StartTotpRequest>,
) -> Result<(StatusCode, Json<TotpSetupResponse>), ApiError> {
    let (ports, staff) = staff.authenticate(&state).await?;
    let req = body.parse()?;
    let setup = start_totp(
        ports.identities(),
        &state.env,
        staff.session.identity(),
        &req.password,
        // Labels the entry in the authenticator app.
        &state.store_name,
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

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CodeRequest {
    /// From the authenticator app, or (to disable or regenerate) a recovery code.
    code: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct RecoveryCodesResponse {
    /// Shown only now: each works once if the authenticator is lost.
    recovery_codes: Vec<String>,
}

/// A wrong code here comes from someone logged in, so it's 422, not the 401 a login
/// gets (a 401 would tell the client its session died).
fn code_error(e: hypha_auth::CommandError) -> ApiError {
    ApiError::from(e).unprocessable_if("invalid_mfa_code")
}

/// Turns 2FA on with a code from the app; returns the recovery codes once.
#[utoipa::path(
    post,
    path = "/admin/me/mfa/totp/confirm",
    request_body = CodeRequest,
    responses((status = 200, description = "OK", body = RecoveryCodesResponse)),
    extensions(("x-hypha-staff-session" = json!(true))),
)]
pub async fn confirm_totp_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    staff: StaffCaller,
    body: JsonBody<CodeRequest>,
) -> Result<Json<RecoveryCodesResponse>, ApiError> {
    let (ports, staff) = staff.authenticate(&state).await?;
    let req = body.parse()?;
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

/// Turns 2FA off; needs a current second-factor code.
#[utoipa::path(
    post,
    path = "/admin/me/mfa/totp/disable",
    request_body = CodeRequest,
    responses((status = 204, description = "No content")),
    extensions(("x-hypha-staff-session" = json!(true))),
)]
pub async fn disable_totp_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    staff: StaffCaller,
    body: JsonBody<CodeRequest>,
) -> Result<StatusCode, ApiError> {
    let (ports, staff) = staff.authenticate(&state).await?;
    let req = body.parse()?;
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

/// Replaces the recovery codes; needs a current second-factor code.
#[utoipa::path(
    post,
    path = "/admin/me/mfa/recovery-codes",
    request_body = CodeRequest,
    responses((status = 200, description = "OK", body = RecoveryCodesResponse)),
    extensions(("x-hypha-staff-session" = json!(true))),
)]
pub async fn recovery_codes_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    staff: StaffCaller,
    body: JsonBody<CodeRequest>,
) -> Result<Json<RecoveryCodesResponse>, ApiError> {
    let (ports, staff) = staff.authenticate(&state).await?;
    let req = body.parse()?;
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
