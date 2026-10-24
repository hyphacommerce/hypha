//! Customers on the storefront: signing in and out (a code is emailed, the code opens a
//! session, the session rides beside the publishable key; rate limited like the staff
//! login, per client address before the body is parsed, per email and address right after),
//! and their account (every route takes the customer from the session,
//! `hypha_customer_accounts::own`, never from input).

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
};
use hypha_core::{
    AddressId, CartId, Clock, CustomerId, Email, Keyset, Notifier, OrderId, SortKey, Stamped,
};
use hypha_custom_fields::{CallerKind, HasCustomFieldDefinitions, HasCustomFieldValues, OwnerType};
use hypha_customer_accounts::{
    CustomerEmail, LoggedIn, Owned, login_with_code, logout, logout_all, own, request_login_code,
};
use hypha_customers::{
    Channel, ConsentChange, ConsentState, Customer, HasCustomers, OptInLevel, UpdateCustomer,
    add_address, remove_address, set_marketing_consent, update_address, update_customer,
};
use hypha_orders::{HasOrders, ListOrders, OrderFilterInput, OrderSort, get_order, list_orders};
use serde::{Deserialize, Serialize};

use super::{
    app_extensions::CART_HEADER,
    auth::session_client,
    custom_fields::{self, CustomFieldValues},
    customers::{AddressRequest, AddressResponse, ConsentResponse},
    orders::{OrderPage, OrderSummaryResponse, PlacedOrderResponse, StorefrontOrderResponse},
    page::{ListQuery, PageParams, PageResponse, SortedFilters},
    payments::PlacedPaymentResponse,
    rfc3339, stamp_times,
};
use crate::{
    client_ip::{ClientAddr, ClientIp},
    error::{ApiError, ApiErrorBody},
    extract::{Caller, CustomerCaller, JsonBody, PathId, Storefront},
    patch,
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
pub struct AccountResponse {
    id: CustomerId,
    /// The address they sign in with; it can't be changed from the account in v1.
    email: Email,
    first_name: Option<String>,
    last_name: Option<String>,
    phone: Option<String>,
    default_shipping_address: Option<AddressId>,
    default_billing_address: Option<AddressId>,
    addresses: Vec<AddressResponse>,
    /// Consent to marketing by email; `PUT /storefront/account/marketing/email` changes it.
    email_marketing: ConsentResponse,
    /// Consent to marketing by SMS; needs a phone.
    sms_marketing: ConsentResponse,
    /// When a requested erasure runs, RFC 3339 in UTC; absent without a request. `DELETE
    /// /storefront/account/erasure` cancels it until then.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    erasure_due_at: Option<String>,
    /// The custom field values the merchant shows the account holder (`access.storefront:
    /// read`), `{"custom.tier": "gold", …}`.
    #[schema(value_type = std::collections::HashMap<String, serde_json::Value>)]
    custom_fields: CustomFieldValues,
    /// When the account was created, RFC 3339 in UTC.
    created_at: String,
}

impl AccountResponse {
    fn new(c: &Customer, custom_fields: CustomFieldValues) -> Result<Self, ApiError> {
        let (created_at, _) = stamp_times(c.stamps())?;
        Ok(Self {
            id: c.id(),
            email: c.email().clone(),
            first_name: c.first_name().map(str::to_string),
            last_name: c.last_name().map(str::to_string),
            phone: c.phone().map(str::to_string),
            default_shipping_address: c.default_shipping(),
            default_billing_address: c.default_billing(),
            addresses: c.addresses().iter().map(AddressResponse::from).collect(),
            email_marketing: ConsentResponse::new(c.marketing_consent(Channel::Email))?,
            sms_marketing: ConsentResponse::new(c.marketing_consent(Channel::Sms))?,
            erasure_due_at: c.erasure_due_at().map(rfc3339).transpose()?,
            custom_fields,
            created_at,
        })
    }
}

/// The account's response for `customer`: with the custom fields the storefront shows
/// (`access.storefront: read`); the holder sees their personal ones too.
async fn respond<P>(
    owned: &Owned,
    ports: &P,
    customer: &Customer,
) -> Result<AccountResponse, ApiError>
where
    P: HasCustomFieldDefinitions + HasCustomFieldValues,
{
    let id = customer.id().as_uuid();
    let fields = custom_fields::read(
        &owned.ctx,
        &CallerKind::Storefront,
        ports,
        &[OwnerType::Customer],
        &[id],
        None,
    )
    .await?;
    AccountResponse::new(customer, fields.json_of(id))
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct CustomerLoginResponse {
    /// Send as `Authorization: Bearer <token>` beside `X-Publishable-Key`. Shown only now.
    token: String,
    /// When the session ends unused; every use within pushes it out again.
    expires_at: String,
    customer: AccountResponse,
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
    // A publishable key always has a channel.
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
    // The session isn't authenticated yet (it was just made): their own context is
    // built from the storefront one, as `CustomerCaller` builds it per request.
    let owned = own(&hypha_core::Ctx::in_channel(
        hypha_core::Actor::Customer(customer.id()),
        hypha_core::Scope::STOREFRONT.to_vec(),
        channel,
    ))?;
    Ok(Json(CustomerLoginResponse {
        token: token.expose().to_string(),
        expires_at: rfc3339(session.expires_at())?,
        customer: respond(&owned, &ports, &customer).await?,
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

// ---------- the account ----------

/// The signed-in customer's account: email, names, phone, address book, marketing
/// consent, a pending erasure, and the custom fields the merchant shows them.
#[utoipa::path(
    get,
    path = "/storefront/account",
    responses((status = 200, description = "OK", body = AccountResponse), (status = 401, description = "No customer session (`account_required`), or one that is invalid or expired (`invalid_token`)", body = ApiErrorBody)),
    extensions(("x-hypha-scopes" = json!([])), ("x-hypha-customer-session" = json!(true))),
)]
pub async fn fetch<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: CustomerCaller,
) -> Result<Json<AccountResponse>, ApiError> {
    let (ports, me) = caller.authenticate(&state).await?;
    let owned = own(&me.ctx)?;
    Ok(Json(respond(&owned, &ports, &me.customer).await?))
}

/// JSON Merge Patch: only the fields sent change, `null` clears one. The email isn't here:
/// it's what the customer signs in with.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateAccountRequest {
    #[serde(default, deserialize_with = "patch::nullable")]
    first_name: Option<Option<String>>,
    #[serde(default, deserialize_with = "patch::nullable")]
    last_name: Option<Option<String>>,
    #[serde(default, deserialize_with = "patch::nullable")]
    phone: Option<Option<String>>,
}

/// Changes the customer's names and phone (the admin's rules: `409 customer_erased`
/// once erased).
#[utoipa::path(
    patch,
    path = "/storefront/account",
    request_body = UpdateAccountRequest,
    responses((status = 200, description = "OK", body = AccountResponse)),
    extensions(("x-hypha-scopes" = json!([])), ("x-hypha-customer-session" = json!(true))),
)]
pub async fn update<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: CustomerCaller,
    body: JsonBody<UpdateAccountRequest>,
) -> Result<Json<AccountResponse>, ApiError> {
    let (ports, me) = caller.authenticate(&state).await?;
    let owned = own(&me.ctx)?;
    let req = body.parse()?;
    let input = UpdateCustomer {
        email: None,
        first_name: req.first_name,
        last_name: req.last_name,
        phone: req.phone,
    };
    let customer = update_customer(&owned.ctx, ports.customers(), owned.customer, input).await?;
    Ok(Json(respond(&owned, &ports, &customer).await?))
}

/// Adds an address to the customer's address book.
#[utoipa::path(
    post,
    path = "/storefront/account/addresses",
    request_body = AddressRequest,
    responses((status = 201, description = "Created", body = AccountResponse)),
    extensions(("x-hypha-scopes" = json!([])), ("x-hypha-customer-session" = json!(true))),
)]
pub async fn add_address_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: CustomerCaller,
    body: JsonBody<AddressRequest>,
) -> Result<(StatusCode, Json<AccountResponse>), ApiError> {
    let (ports, me) = caller.authenticate(&state).await?;
    let owned = own(&me.ctx)?;
    let req = body.parse()?;
    let customer = add_address(
        &owned.ctx,
        ports.customers(),
        &state.ids,
        owned.customer,
        req.into(),
    )
    .await?;
    Ok((
        StatusCode::CREATED,
        Json(respond(&owned, &ports, &customer).await?),
    ))
}

/// Replaces a saved address, defaults included.
#[utoipa::path(
    put,
    path = "/storefront/account/addresses/{address_id}",
    params(("address_id" = AddressId, Path)),
    request_body = AddressRequest,
    responses((status = 200, description = "OK", body = AccountResponse)),
    extensions(("x-hypha-scopes" = json!([])), ("x-hypha-customer-session" = json!(true))),
)]
pub async fn update_address_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: CustomerCaller,
    PathId(address): PathId<AddressId>,
    body: JsonBody<AddressRequest>,
) -> Result<Json<AccountResponse>, ApiError> {
    let (ports, me) = caller.authenticate(&state).await?;
    let owned = own(&me.ctx)?;
    let req = body.parse()?;
    let customer = update_address(
        &owned.ctx,
        ports.customers(),
        owned.customer,
        address,
        req.into(),
    )
    .await?;
    Ok(Json(respond(&owned, &ports, &customer).await?))
}

/// Removes a saved address.
#[utoipa::path(
    delete,
    path = "/storefront/account/addresses/{address_id}",
    params(("address_id" = AddressId, Path)),
    responses((status = 200, description = "OK", body = AccountResponse)),
    extensions(("x-hypha-scopes" = json!([])), ("x-hypha-customer-session" = json!(true))),
)]
pub async fn remove_address_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: CustomerCaller,
    PathId(address): PathId<AddressId>,
) -> Result<Json<AccountResponse>, ApiError> {
    let (ports, me) = caller.authenticate(&state).await?;
    let owned = own(&me.ctx)?;
    let customer = remove_address(&owned.ctx, ports.customers(), owned.customer, address).await?;
    Ok(Json(respond(&owned, &ports, &customer).await?))
}

/// What the customer says about marketing. The time is now and the source `storefront`:
/// it's the person themselves.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetAccountConsentRequest {
    /// `subscribed` or `unsubscribed` (`pending` is for double opt-in flows an app runs).
    #[schema(value_type = String)]
    state: ConsentState,
}

/// A consent write's outcome: whether it changed anything, and the account as it is.
#[derive(Serialize, utoipa::ToSchema)]
pub struct AccountConsentResponse {
    /// `false` when nothing changed.
    applied: bool,
    account: AccountResponse,
}

/// Records the customer's own marketing consent for a channel (`email` or `sms`), from
/// the storefront, now. SMS needs a phone (`409 consent_needs_phone`).
#[utoipa::path(
    put,
    path = "/storefront/account/marketing/{channel}",
    params(("channel" = String, Path, description = "`email` or `sms`", example = "email")),
    request_body = SetAccountConsentRequest,
    responses((status = 200, description = "OK", body = AccountConsentResponse)),
    extensions(("x-hypha-scopes" = json!([])), ("x-hypha-customer-session" = json!(true))),
)]
pub async fn set_consent<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: CustomerCaller,
    PathId(channel): PathId<String>,
    body: JsonBody<SetAccountConsentRequest>,
) -> Result<Json<AccountConsentResponse>, ApiError> {
    let (ports, me) = caller.authenticate(&state).await?;
    let owned = own(&me.ctx)?;
    let Some(channel) = Channel::parse(&channel) else {
        return Err(ApiError::route_not_found());
    };
    let req = body.parse()?;
    let change = ConsentChange {
        state: req.state,
        opt_in_level: OptInLevel::Single,
        consented_at: None,
    };
    let (customer, applied) = set_marketing_consent(
        &owned.ctx,
        ports.customers(),
        &state.clock,
        owned.customer,
        channel,
        change,
    )
    .await?;
    Ok(Json(AccountConsentResponse {
        applied,
        account: respond(&owned, &ports, &customer).await?,
    }))
}

/// A list of the account's orders takes no filters: it's always theirs, newest first.
#[derive(Deserialize, utoipa::IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct AccountOrderFilters {}

impl SortedFilters for AccountOrderFilters {
    fn fits(&self, key: &SortKey) -> bool {
        OrderSort::Newest.fits(key)
    }
}

/// The customer's orders, newest first, from every channel of the store.
#[utoipa::path(
    get,
    path = "/storefront/account/orders",
    params(PageParams, AccountOrderFilters),
    responses((status = 200, description = "OK", body = OrderPage)),
    extensions(("x-hypha-scopes" = json!([])), ("x-hypha-customer-session" = json!(true))),
)]
pub async fn list_orders_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: CustomerCaller,
    query: ListQuery<AccountOrderFilters>,
) -> Result<Json<OrderPage>, ApiError> {
    let (ports, me) = caller.authenticate(&state).await?;
    let owned = own(&me.ctx)?;
    let (page, _, scope) = query.parse_sorted()?;
    let input = ListOrders {
        filter: OrderFilterInput {
            customer: Some(owned.customer),
            ..OrderFilterInput::default()
        },
        sort: OrderSort::Newest,
        fields: Vec::new(),
    };
    let page = list_orders(&owned.ctx, ports.orders(), input, page).await?;
    let items = page
        .items
        .iter()
        .cloned()
        .map(|o| OrderSummaryResponse::new(o, None))
        .collect::<Result<_, _>>()?;
    Ok(Json(PageResponse::sorted(
        &page,
        &scope,
        |o| Keyset {
            key: SortKey::Id,
            id: o.id,
        },
        items,
    )?))
}

/// One of the customer's orders, as the confirmation page shows it, with its access
/// token (so a link to the order page works); someone else's is `404 order_not_found`.
#[utoipa::path(
    get,
    path = "/storefront/account/orders/{id}",
    params(("id" = OrderId, Path)),
    responses((status = 200, description = "OK", body = PlacedOrderResponse)),
    extensions(("x-hypha-scopes" = json!([])), ("x-hypha-customer-session" = json!(true))),
)]
pub async fn fetch_order<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: CustomerCaller,
    PathId(id): PathId<OrderId>,
) -> Result<Json<PlacedOrderResponse>, ApiError> {
    let (ports, me) = caller.authenticate(&state).await?;
    let owned = own(&me.ctx)?;
    let order = get_order(&owned.ctx, ports.orders(), id).await?;
    if order.customer() != owned.customer {
        return Err(hypha_orders::CommandError::OrderNotFound(id).into());
    }
    let payment = hypha_payments::newest_payment(&ports, id).await?;
    Ok(Json(PlacedOrderResponse {
        order: StorefrontOrderResponse::with_images(&order, &ports, state.media.as_deref()).await?,
        access_token: state.env.order_token(id),
        payment: payment.as_ref().map(PlacedPaymentResponse::new),
    }))
}

/// Everything Hypha holds about the customer, for the person it's about: the account, their
/// orders, the orders' payments and the emails sent about them. Records
/// `customer.data_requested`, as the admin's export does.
#[derive(Serialize, utoipa::ToSchema)]
pub struct AccountExportResponse {
    /// When the export was made, RFC 3339 in UTC.
    exported_at: String,
    account: AccountResponse,
    orders: Vec<StorefrontOrderResponse>,
    payments: Vec<super::payments::PaymentResponse>,
    /// The emails sent about the orders.
    emails: Vec<super::notifications::NotificationResponse>,
}

/// The customer's data, as one JSON document.
#[utoipa::path(
    get,
    path = "/storefront/account/export",
    responses((status = 200, description = "OK", body = AccountExportResponse)),
    extensions(("x-hypha-scopes" = json!([])), ("x-hypha-customer-session" = json!(true))),
)]
pub async fn export<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: CustomerCaller,
) -> Result<Json<AccountExportResponse>, ApiError> {
    let (ports, me) = caller.authenticate(&state).await?;
    let owned = own(&me.ctx)?;
    let export = hypha_privacy::export_customer(&owned.ctx, &ports, owned.customer).await?;
    Ok(Json(AccountExportResponse {
        exported_at: rfc3339(state.clock.now())?,
        account: respond(&owned, &ports, &export.customer).await?,
        orders: export
            .orders
            .iter()
            .map(StorefrontOrderResponse::new)
            .collect::<Result<_, _>>()?,
        payments: export
            .payments
            .iter()
            .map(super::payments::PaymentResponse::new)
            .collect::<Result<_, _>>()?,
        emails: export
            .notifications
            .iter()
            .map(super::notifications::NotificationResponse::new)
            .collect::<Result<_, _>>()?,
    }))
}

/// Asks for the customer's own personal data to be erased after the store's cancel window
/// (`customers.erasure_delay`), recording `customer.erasure_requested` and emailing when it
/// runs (`customer_erasure_requested`). Signing in still works until then, to cancel. `409
/// erasure_already_requested` while one is pending.
#[utoipa::path(
    post,
    path = "/storefront/account/erasure",
    responses((status = 200, description = "OK", body = AccountResponse)),
    extensions(("x-hypha-scopes" = json!([])), ("x-hypha-customer-session" = json!(true))),
)]
pub async fn request_erasure_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: CustomerCaller,
) -> Result<Json<AccountResponse>, ApiError> {
    let (ports, me) = caller.authenticate(&state).await?;
    let owned = own(&me.ctx)?;
    let customer = hypha_privacy::request_erasure(
        &owned.ctx,
        &ports,
        &state.clock,
        &state.config.privacy,
        owned.customer,
    )
    .await?;
    if let Some(due_at) = customer.erasure_due_at()
        && let Err(e) = state
            .staff_env(&ports)
            .notify(CustomerEmail::ErasureRequested {
                to: customer.email().clone(),
                due_at,
            })
            .await
    {
        // The request stands (the account shows when it runs); the email is a courtesy.
        tracing::warn!(customer = %customer.id(), error = %e, "erasure confirmation not sent");
    }
    Ok(Json(respond(&owned, &ports, &customer).await?))
}

/// Cancels a requested erasure while its window is open (`409 erasure_not_requested`
/// otherwise), recording `customer.erasure_cancelled`.
#[utoipa::path(
    delete,
    path = "/storefront/account/erasure",
    responses((status = 200, description = "OK", body = AccountResponse)),
    extensions(("x-hypha-scopes" = json!([])), ("x-hypha-customer-session" = json!(true))),
)]
pub async fn cancel_erasure_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: CustomerCaller,
) -> Result<Json<AccountResponse>, ApiError> {
    let (ports, me) = caller.authenticate(&state).await?;
    let owned = own(&me.ctx)?;
    let customer = hypha_privacy::cancel_erasure(&owned.ctx, &ports, owned.customer).await?;
    Ok(Json(respond(&owned, &ports, &customer).await?))
}
