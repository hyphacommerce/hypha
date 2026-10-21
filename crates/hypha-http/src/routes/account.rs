//! Customers signing in and out on the storefront (design note 0034, §2): a code is
//! emailed, the code opens a session, the session rides beside the publishable key.
//! Rate limited like the staff login (design note 0009): per client address before the
//! body is parsed, per email and address right after.

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
};
use hypha_core::{CartId, Email};
use hypha_customer_accounts::{LoggedIn, login_with_code, logout, logout_all, request_login_code};
use hypha_customers::Customer;
use serde::{Deserialize, Serialize};

use super::{app_extensions::CART_HEADER, auth::session_client, rfc3339};
use crate::{
    client_ip::{ClientAddr, ClientIp},
    error::{ApiError, ApiErrorBody},
    extract::{Caller, CustomerCaller, JsonBody, Storefront},
    rate_limit::{Rule, enforce, pair_key},
    state::{SharedState, Storage},
};

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct LoginCodeRequest {
    email: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct LoginCodeResponse {
    /// Always `true`: the route answers the same whatever the email, so it tells nobody
    /// who is a customer. The email goes out only to an address that can take one.
    sent: bool,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CustomerLoginRequest {
    email: String,
    /// The six digits from the email.
    code: String,
}

/// The signed-in customer, as they see themselves.
#[derive(Serialize, utoipa::ToSchema)]
pub struct AccountCustomer {
    id: String,
    email: String,
    first_name: Option<String>,
    last_name: Option<String>,
}

impl AccountCustomer {
    pub fn new(customer: &Customer) -> Self {
        Self {
            id: customer.id().to_string(),
            email: customer.email().to_string(),
            first_name: customer.first_name().map(str::to_string),
            last_name: customer.last_name().map(str::to_string),
        }
    }
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct CustomerLoginResponse {
    /// Send as `Authorization: Bearer <token>` beside `X-Publishable-Key`. Shown only now.
    token: String,
    /// When the session ends unused; every use within pushes it out again.
    expires_at: String,
    customer: AccountCustomer,
}

/// Emails a sign-in code. 202 whatever the email: an address that isn't a customer's
/// yet becomes one (the customer is created), one that can't take email gets nothing,
/// and asking again within a minute sends nothing. 429 `rate_limited` per client
/// address and per email and address.
#[utoipa::path(
    post,
    path = "/storefront/auth/code",
    request_body = LoginCodeRequest,
    responses((status = 202, description = "Accepted", body = LoginCodeResponse), (status = 429, description = "Over the rate limit (`rate_limited`); see `Retry-After`", body = ApiErrorBody), (status = 502, description = "The code could not be emailed (`notification_failed`)", body = ApiErrorBody)),
    extensions(("x-hypha-scopes" = json!([]))),
)]
pub async fn request_code<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller<Storefront>,
    ClientIp(ip): ClientIp,
    body: JsonBody<LoginCodeRequest>,
) -> Result<(StatusCode, Json<LoginCodeResponse>), ApiError> {
    let (ports, _ctx) = caller.authenticate(&state).await?;
    enforce(&state.limiter, Rule::CodeIp, &ip).await?;
    let req = body.parse()?;
    if let Ok(email) = Email::new(&req.email) {
        enforce(
            &state.limiter,
            Rule::CodeEmailFromIp,
            &pair_key(&email, &ip),
        )
        .await?;
    }
    request_login_code(&ports, &state.staff_env(&ports), &req.email).await?;
    Ok((StatusCode::ACCEPTED, Json(LoginCodeResponse { sent: true })))
}

/// Signs in with the emailed code and answers the session token, shown once. 401
/// `invalid_code` for a wrong, expired or used-up code, deliberately one answer. With an
/// `X-Hypha-Cart: cart_…` header (the shopper's cart in this channel), the cart is
/// linked to the customer.
#[utoipa::path(
    post,
    path = "/storefront/auth/login",
    params(("X-Hypha-Cart" = Option<String>, Header, description = "The shopper's cart (`cart_…`), linked to the customer on signing in.")),
    request_body = CustomerLoginRequest,
    responses((status = 200, description = "OK", body = CustomerLoginResponse), (status = 401, description = "Wrong, expired or used-up code (`invalid_code`)", body = ApiErrorBody), (status = 429, description = "Over the rate limit (`rate_limited`); see `Retry-After`", body = ApiErrorBody)),
    extensions(("x-hypha-scopes" = json!([]))),
)]
pub async fn login<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller<Storefront>,
    ClientIp(ip): ClientIp,
    addr: ClientAddr,
    headers: HeaderMap,
    body: JsonBody<CustomerLoginRequest>,
) -> Result<Json<CustomerLoginResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    enforce(&state.limiter, Rule::CustomerLoginIp, &ip).await?;
    let req = body.parse()?;
    if let Ok(email) = Email::new(&req.email) {
        enforce(
            &state.limiter,
            Rule::CustomerLoginEmailFromIp,
            &pair_key(&email, &ip),
        )
        .await?;
    }
    let cart = match headers.get(CART_HEADER) {
        Some(raw) => Some(
            raw.to_str()
                .ok()
                .and_then(|s| s.trim().parse::<CartId>().ok())
                .ok_or_else(ApiError::invalid_cart_header)?,
        ),
        None => None,
    };
    // A publishable key always has a channel (design note 0020).
    let channel = ctx
        .channel()
        .ok_or(hypha_auth::CommandError::InvalidToken)?;
    let client = session_client(&addr, &headers);
    let LoggedIn {
        session,
        token,
        customer,
    } = login_with_code(
        &ports,
        &state.staff_env(&ports),
        channel,
        &req.email,
        &req.code,
        &client,
        cart,
    )
    .await?;
    Ok(Json(CustomerLoginResponse {
        token: token.expose().to_string(),
        expires_at: rfc3339(session.expires_at())?,
        customer: AccountCustomer::new(&customer),
    }))
}

/// Ends the customer's session.
#[utoipa::path(
    post,
    path = "/storefront/account/logout",
    responses((status = 204, description = "No content"), (status = 401, description = "No customer session (`account_required`), or one that is invalid or expired (`invalid_token`)", body = ApiErrorBody)),
    extensions(("x-hypha-scopes" = json!([])), ("x-hypha-customer-session" = json!(true))),
)]
pub async fn logout_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: CustomerCaller,
) -> Result<StatusCode, ApiError> {
    let (ports, me) = caller.authenticate(&state).await?;
    logout(&ports, &state.clock, me.session).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Ends every session of the customer, on every device, this one included.
#[utoipa::path(
    post,
    path = "/storefront/account/logout-all",
    responses((status = 204, description = "No content"), (status = 401, description = "No customer session (`account_required`), or one that is invalid or expired (`invalid_token`)", body = ApiErrorBody)),
    extensions(("x-hypha-scopes" = json!([])), ("x-hypha-customer-session" = json!(true))),
)]
pub async fn logout_all_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: CustomerCaller,
) -> Result<StatusCode, ApiError> {
    let (ports, me) = caller.authenticate(&state).await?;
    logout_all(&ports, &state.clock, &me.session).await?;
    Ok(StatusCode::NO_CONTENT)
}
