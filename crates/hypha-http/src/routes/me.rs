//! The logged-in staff member's own account: who am I, password, two-factor setup
//! (design note 0008, section 6). Each route needs a session, not an API key.

use axum::{Json, extract::State, http::StatusCode};
use hypha_auth::{
    HasIdentities, HasSessions, IdentityRepo, Session, change_password, confirm_totp, disable_mfa,
    list_sessions, regenerate_recovery_codes, revoke_other_sessions, revoke_session, start_totp,
};
use hypha_core::{RoleId, Scope, SessionId};
use hypha_users::{HasUsers, StaffSession, rename_me};
use serde::{Deserialize, Serialize};

use super::{rfc3339, staff::UserResponse};
use crate::{
    error::ApiError,
    extract::{JsonBody, PathId, StaffCaller},
    patch,
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
    me_response(&ports, &staff).await.map(Json)
}

async fn me_response(
    ports: &impl HasIdentities,
    staff: &StaffSession,
) -> Result<MeResponse, ApiError> {
    let identity = ports
        .identities()
        .get(staff.session.identity())
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::internal("session without identity"))?;
    Ok(MeResponse {
        user: UserResponse::new(&staff.user)?,
        role: MeRole {
            id: staff.role.id(),
            name: staff.role.name().to_string(),
        },
        scopes: staff.role.granted(),
        mfa_enabled: identity.mfa_enabled(),
    })
}

/// JSON Merge Patch on the caller's own profile: only the fields sent change.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateMeRequest {
    #[serde(default, deserialize_with = "patch::non_null")]
    // Optional, but `null` is refused (`patch::non_null`): not nullable in the spec.
    #[schema(value_type = String, required = false)]
    name: Option<String>,
}

/// Changes the caller's own name. Needs no scope: everyone may name themselves
/// (changing someone else is `PATCH /admin/users/{id}`, with `users:write`).
#[utoipa::path(
    patch,
    path = "/admin/me",
    request_body = UpdateMeRequest,
    responses((status = 200, description = "OK", body = MeResponse)),
    extensions(("x-hypha-staff-session" = json!(true))),
)]
pub async fn update<S: Storage>(
    State(state): State<SharedState<S>>,
    staff: StaffCaller,
    body: JsonBody<UpdateMeRequest>,
) -> Result<Json<MeResponse>, ApiError> {
    let (ports, mut staff) = staff.authenticate(&state).await?;
    let req = body.parse()?;
    if let Some(name) = &req.name {
        staff.user = rename_me(ports.users(), staff.user.id(), name).await?;
    }
    me_response(&ports, &staff).await.map(Json)
}

// ---------- sessions ----------

#[derive(Serialize, utoipa::ToSchema)]
pub struct MySessionResponse {
    id: SessionId,
    /// Whether this is the session making the request.
    current: bool,
    /// When the login happened.
    created_at: String,
    /// When a request last used it, to within 15 minutes.
    last_used_at: String,
    expires_at: String,
    /// The address the login came from, as the API saw it.
    ip: Option<String>,
    /// The login's `User-Agent`, for telling browsers apart.
    user_agent: Option<String>,
}

impl MySessionResponse {
    fn new(s: &Session, current: SessionId) -> Result<Self, ApiError> {
        Ok(Self {
            id: s.id(),
            current: s.id() == current,
            created_at: rfc3339(s.created_at())?,
            last_used_at: rfc3339(s.last_used_at())?,
            expires_at: rfc3339(s.expires_at())?,
            ip: s.client().ip().map(str::to_string),
            user_agent: s.client().user_agent().map(str::to_string),
        })
    }
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct MySessionListResponse {
    /// Newest first. A person has a handful, so they come in one list.
    items: Vec<MySessionResponse>,
}

/// The caller's signed-in sessions, this one included (`current`), newest first.
#[utoipa::path(
    get,
    path = "/admin/me/sessions",
    responses((status = 200, description = "OK", body = MySessionListResponse)),
    extensions(("x-hypha-staff-session" = json!(true))),
)]
pub async fn list_sessions_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    staff: StaffCaller,
) -> Result<Json<MySessionListResponse>, ApiError> {
    let (ports, staff) = staff.authenticate(&state).await?;
    let sessions = list_sessions(ports.sessions(), &state.clock, &staff.session).await?;
    Ok(Json(MySessionListResponse {
        items: sessions
            .iter()
            .map(|s| MySessionResponse::new(s, staff.session.id()))
            .collect::<Result<_, _>>()?,
    }))
}

/// Signs one of the caller's sessions out (their current one too: that's a logout).
/// Anyone else's session is 404 `session_not_found`.
#[utoipa::path(
    delete,
    path = "/admin/me/sessions/{id}",
    params(("id" = SessionId, Path)),
    responses((status = 204, description = "No content")),
    extensions(("x-hypha-staff-session" = json!(true))),
)]
pub async fn revoke_session_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    staff: StaffCaller,
    PathId(id): PathId<SessionId>,
) -> Result<StatusCode, ApiError> {
    let (ports, staff) = staff.authenticate(&state).await?;
    revoke_session(ports.sessions(), &state.clock, &staff.session, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Signs out every session of the caller's but this one ("sign out everywhere else").
#[utoipa::path(
    post,
    path = "/admin/me/sessions/revoke-others",
    responses((status = 204, description = "No content")),
    extensions(("x-hypha-staff-session" = json!(true))),
)]
pub async fn revoke_other_sessions_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    staff: StaffCaller,
) -> Result<StatusCode, ApiError> {
    let (ports, staff) = staff.authenticate(&state).await?;
    revoke_other_sessions(&ports, &state.clock, &staff.session).await?;
    Ok(StatusCode::NO_CONTENT)
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
