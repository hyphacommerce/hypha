//! Logging in and out, password resets and accepting invites (design note 0008,
//! sections 3 and 5). These routes take no `Caller`: they are how one gets credentials.
//! So they are rate limited (design note 0009): per client IP before the body is even
//! parsed, per email right after, and both before any hashing, storage or email.

use axum::{Json, extract::State, http::StatusCode};
use hypha_auth::{
    HasSessions, LoginStep, MfaMethod, NewToken, Session, finish_login, logout, reset_password,
};
use hypha_core::Email;
use hypha_users::{AcceptInvite, accept_invite, login, request_password_reset};
use serde::{Deserialize, Serialize};

use super::{rfc3339, staff::UserResponse};
use crate::{
    client_ip::ClientIp,
    error::{ApiError, ApiErrorBody},
    extract::{JsonBody, StaffCaller},
    rate_limit::{Rule, enforce, pair_key},
    state::{SharedState, Storage},
};

/// The email as rate limits key it: `Email`'s own normalisation (so one account is one
/// key, the same as at login) and its length cap (so a key can't be megabytes). `None`
/// for something that isn't an email: it can't match an account, so no email limit
/// applies, while the address limit still does.
fn limit_email(raw: &str) -> Option<Email> {
    Email::new(raw).ok()
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct LoginRequest {
    email: String,
    password: String,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct FinishLoginRequest {
    /// The challenge token from the first step.
    challenge: String,
    /// A 6-digit code from the authenticator app, or a recovery code.
    code: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct SessionResponse {
    /// Send as `Authorization: Bearer <token>`. Shown only now.
    token: String,
    expires_at: String,
}

impl SessionResponse {
    fn new(session: &Session, token: &NewToken) -> Result<Self, ApiError> {
        Ok(Self {
            token: token.expose().to_string(),
            expires_at: rfc3339(session.expires_at())?,
        })
    }
}

#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct ChallengeResponse {
    /// Send back to `POST /admin/auth/login/mfa` with a code.
    token: String,
    methods: Vec<&'static str>,
    expires_at: String,
}

/// Either `{"status": "authenticated", "session": …}` or `{"status": "mfa_required",
/// "challenge": …}`: a tagged enum, so a client switches on `status`.
#[derive(Serialize, utoipa::ToSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(super) enum LoginResponse {
    Authenticated { session: SessionResponse },
    MfaRequired { challenge: ChallengeResponse },
}

/// The first login step. 401 `invalid_credentials` for a wrong email or password (or a
/// disabled account), deliberately the same answer.
#[utoipa::path(
    post,
    path = "/admin/auth/login",
    request_body = LoginRequest,
    responses((status = 200, description = "OK", body = LoginResponse), (status = 401, description = "Wrong email or password, or a disabled account: one answer for all (`invalid_credentials`).", body = ApiErrorBody), (status = 429, description = "Over a rate limit (`rate_limited`); see `Retry-After`", body = ApiErrorBody)),
    security(()),
)]
pub(super) async fn login_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    ClientIp(ip): ClientIp,
    body: JsonBody<LoginRequest>,
) -> Result<Json<LoginResponse>, ApiError> {
    enforce(&state.limiter, Rule::LoginIp, &ip).await?;
    // Parsed before taking a pooled connection: a malformed body costs none.
    let req = body.parse()?;
    if let Some(email) = limit_email(&req.email) {
        enforce(
            &state.limiter,
            Rule::LoginEmailFromIp,
            &pair_key(&email, &ip),
        )
        .await?;
        enforce(&state.limiter, Rule::LoginEmail, email.as_str()).await?;
    }
    let ports = state.public_ports().await?;
    let step = login(&ports, &state.env, &req.email, &req.password).await?;
    Ok(Json(match step {
        LoginStep::Authenticated { session, token } => LoginResponse::Authenticated {
            session: SessionResponse::new(&session, &token)?,
        },
        LoginStep::MfaRequired {
            session,
            token,
            methods,
        } => LoginResponse::MfaRequired {
            challenge: ChallengeResponse {
                token: token.expose().to_string(),
                methods: methods.iter().map(|m: &MfaMethod| m.as_str()).collect(),
                expires_at: rfc3339(session.expires_at())?,
            },
        },
    }))
}

/// The second step, when the first answered `mfa_required`. A wrong code is 401
/// `invalid_mfa_code`; the fifth wrong code ends the challenge (then `invalid_token`).
#[utoipa::path(
    post,
    path = "/admin/auth/login/mfa",
    request_body = FinishLoginRequest,
    responses((status = 200, description = "OK", body = LoginResponse), (status = 401, description = "A wrong or used code (`invalid_mfa_code`), or an unknown, expired or ended challenge (`invalid_token`).", body = ApiErrorBody), (status = 429, description = "Over a rate limit (`rate_limited`, see `Retry-After`), or second-factor checks locked after five wrong codes (`mfa_locked`, see `details.locked_until`).", body = ApiErrorBody)),
    security(()),
)]
pub(super) async fn finish_login_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    ClientIp(ip): ClientIp,
    body: JsonBody<FinishLoginRequest>,
) -> Result<Json<LoginResponse>, ApiError> {
    enforce(&state.limiter, Rule::MfaIp, &ip).await?;
    let req = body.parse()?;
    let ports = state.public_ports().await?;
    let (session, token) = finish_login(&ports, &state.env, &req.challenge, &req.code).await?;
    Ok(Json(LoginResponse::Authenticated {
        session: SessionResponse::new(&session, &token)?,
    }))
}

/// Ends the caller's session.
#[utoipa::path(
    post,
    path = "/admin/auth/logout",
    responses((status = 204, description = "No content")),
    extensions(("x-hypha-staff-session" = json!(true))),
)]
pub(super) async fn logout_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    staff: StaffCaller,
) -> Result<StatusCode, ApiError> {
    let (ports, staff) = staff.authenticate(&state).await?;
    logout(ports.sessions(), &state.clock, staff.session).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct PasswordResetRequest {
    email: String,
}

/// 202 at once, whether or not the email belongs to anyone: the work (finding the
/// account, storing the reset, sending the email) runs on its own task, so neither the
/// answer nor its timing tells which emails exist. The one other answer is 429
/// `rate_limited` (design note 0009), which depends on the caller's address and the
/// email asked about, never on whether the account exists.
#[utoipa::path(
    post,
    path = "/admin/auth/password-reset",
    request_body = PasswordResetRequest,
    responses((status = 202, description = "Accepted"), (status = 429, description = "Over a rate limit (`rate_limited`); see `Retry-After`", body = ApiErrorBody)),
    security(()),
)]
pub(super) async fn request_reset_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    ClientIp(ip): ClientIp,
    body: JsonBody<PasswordResetRequest>,
) -> Result<StatusCode, ApiError> {
    enforce(&state.limiter, Rule::ResetIp, &ip).await?;
    let req = body.parse()?;
    if let Some(email) = limit_email(&req.email) {
        enforce(
            &state.limiter,
            Rule::ResetEmailFromIp,
            &pair_key(&email, &ip),
        )
        .await?;
    }
    S::spawn_password_reset(state, req.email);
    Ok(StatusCode::ACCEPTED)
}

/// The work behind `request_reset_handler`; failures can only be logged.
pub(crate) async fn password_reset_in_background<S: Storage>(state: SharedState<S>, email: String) {
    let result = match state.public_ports().await {
        Ok(ports) => request_password_reset(&ports, &state.env, &email)
            .await
            .map_err(|e| e.to_string()),
        Err(_) => Err("cannot reach storage".to_string()),
    };
    if let Err(error) = result {
        tracing::error!(%error, "password reset request failed");
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct ConfirmResetRequest {
    token: String,
    password: String,
}

/// Sets the new password and logs out every session of the account.
#[utoipa::path(
    post,
    path = "/admin/auth/password-reset/confirm",
    request_body = ConfirmResetRequest,
    responses((status = 204, description = "No content"), (status = 401, description = "An unknown, expired or used reset token (`invalid_token`).", body = ApiErrorBody), (status = 429, description = "Over a rate limit (`rate_limited`); see `Retry-After`", body = ApiErrorBody)),
    security(()),
)]
pub(super) async fn confirm_reset_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    ClientIp(ip): ClientIp,
    body: JsonBody<ConfirmResetRequest>,
) -> Result<StatusCode, ApiError> {
    enforce(&state.limiter, Rule::ResetConfirmIp, &ip).await?;
    let req = body.parse()?;
    let ports = state.public_ports().await?;
    reset_password(&ports, &state.env, &req.token, &req.password).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct AcceptInviteRequest {
    token: String,
    name: String,
    password: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(super) struct AcceptedResponse {
    user: UserResponse,
    session: SessionResponse,
}

/// Creates the invited user and logs them in.
#[utoipa::path(
    post,
    path = "/admin/invites/accept",
    request_body = AcceptInviteRequest,
    responses((status = 201, description = "Created", body = AcceptedResponse), (status = 401, description = "An unknown, expired or accepted invite token (`invalid_token`).", body = ApiErrorBody), (status = 429, description = "Over a rate limit (`rate_limited`); see `Retry-After`", body = ApiErrorBody)),
    security(()),
)]
pub(super) async fn accept_invite_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    ClientIp(ip): ClientIp,
    body: JsonBody<AcceptInviteRequest>,
) -> Result<(StatusCode, Json<AcceptedResponse>), ApiError> {
    enforce(&state.limiter, Rule::InviteIp, &ip).await?;
    let req = body.parse()?;
    let ports = state.public_ports().await?;
    let input = AcceptInvite {
        token: req.token,
        name: req.name,
        password: req.password,
    };
    let (user, session, token) = accept_invite(&ports, &state.env, input).await?;
    let response = AcceptedResponse {
        user: UserResponse::new(&user)?,
        session: SessionResponse::new(&session, &token)?,
    };
    Ok((StatusCode::CREATED, Json(response)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_email_key_is_normalised_like_a_login() {
        assert_eq!(
            limit_email(" Ana@Example.COM ").map(|e| e.as_str().to_string()),
            Some("ana@example.com".to_string())
        );
    }

    #[test]
    fn nothing_longer_than_an_email_becomes_a_key() {
        let long = format!("{}@example.com", "x".repeat(Email::MAX_LEN));
        assert_eq!(limit_email(&long), None);
        assert_eq!(limit_email("not an email"), None);
    }
}
