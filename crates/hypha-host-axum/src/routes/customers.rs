//! Customers and their address books (all live in `hypha-customers`).

use axum::{Json, extract::State, http::StatusCode};
use hypha_core::{AddressId, AddressParts, CountryCode, CustomerId, Email};
use hypha_customers::{
    CreateCustomer, Customer, HasCustomers, SaveAddress, SavedAddress, UpdateCustomer, add_address,
    create_customer, get_customer, remove_address, update_address, update_customer,
};
use serde::{Deserialize, Serialize};

use crate::{
    error::ApiError,
    extract::{Caller, JsonBody, PathId},
    patch,
    state::{SharedState, Storage},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CreateCustomerRequest {
    email: String,
    first_name: Option<String>,
    last_name: Option<String>,
    phone: Option<String>,
}

/// JSON Merge Patch (see `patch`): only the fields sent change, `null` clears a name or
/// phone. The email can be changed but not cleared.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdateCustomerRequest {
    #[serde(default, deserialize_with = "patch::non_null")]
    email: Option<String>,
    #[serde(default, deserialize_with = "patch::nullable")]
    first_name: Option<Option<String>>,
    #[serde(default, deserialize_with = "patch::nullable")]
    last_name: Option<Option<String>>,
    #[serde(default, deserialize_with = "patch::nullable")]
    phone: Option<Option<String>>,
}

/// An address for the address book. Sending it again (PUT) replaces it whole,
/// including whether it is a default.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AddressRequest {
    first_name: Option<String>,
    last_name: String,
    company: Option<String>,
    line1: String,
    line2: Option<String>,
    city: String,
    province: Option<String>,
    postal_code: Option<String>,
    country: String,
    phone: Option<String>,
    #[serde(default)]
    default_shipping: bool,
    #[serde(default)]
    default_billing: bool,
}

impl From<AddressRequest> for SaveAddress {
    fn from(r: AddressRequest) -> Self {
        SaveAddress {
            address: AddressParts {
                first_name: r.first_name,
                last_name: r.last_name,
                company: r.company,
                line1: r.line1,
                line2: r.line2,
                city: r.city,
                province: r.province,
                postal_code: r.postal_code,
                country: r.country,
                phone: r.phone,
            },
            default_shipping: r.default_shipping,
            default_billing: r.default_billing,
        }
    }
}

#[derive(Serialize)]
pub(super) struct CustomerResponse {
    id: CustomerId,
    email: Email,
    first_name: Option<String>,
    last_name: Option<String>,
    phone: Option<String>,
    default_shipping_address: Option<AddressId>,
    default_billing_address: Option<AddressId>,
    addresses: Vec<AddressResponse>,
}

#[derive(Serialize)]
pub(super) struct AddressResponse {
    id: AddressId,
    first_name: Option<String>,
    last_name: String,
    company: Option<String>,
    line1: String,
    line2: Option<String>,
    city: String,
    province: Option<String>,
    postal_code: Option<String>,
    country: CountryCode,
    phone: Option<String>,
}

impl From<&SavedAddress> for AddressResponse {
    fn from(saved: &SavedAddress) -> Self {
        let p = saved.address.parts();
        Self {
            id: saved.id,
            first_name: p.first_name.clone(),
            last_name: p.last_name.clone(),
            company: p.company.clone(),
            line1: p.line1.clone(),
            line2: p.line2.clone(),
            city: p.city.clone(),
            province: p.province.clone(),
            postal_code: p.postal_code.clone(),
            country: saved.address.country(),
            phone: p.phone.clone(),
        }
    }
}

impl From<&Customer> for CustomerResponse {
    fn from(c: &Customer) -> Self {
        Self {
            id: c.id(),
            email: c.email().clone(),
            first_name: c.first_name().map(str::to_string),
            last_name: c.last_name().map(str::to_string),
            phone: c.phone().map(str::to_string),
            default_shipping_address: c.default_shipping(),
            default_billing_address: c.default_billing(),
            addresses: c.addresses().iter().map(AddressResponse::from).collect(),
        }
    }
}

pub(super) async fn create<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    body: JsonBody<CreateCustomerRequest>,
) -> Result<(StatusCode, Json<CustomerResponse>), ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let input = CreateCustomer {
        email: req.email,
        first_name: req.first_name,
        last_name: req.last_name,
        phone: req.phone,
    };
    let customer = create_customer(&ctx, ports.customers(), &state.ids, input).await?;
    Ok((StatusCode::CREATED, Json(CustomerResponse::from(&customer))))
}

pub(super) async fn fetch<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<CustomerId>,
) -> Result<Json<CustomerResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let customer = get_customer(&ctx, ports.customers(), id).await?;
    Ok(Json(CustomerResponse::from(&customer)))
}

pub(super) async fn update<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<CustomerId>,
    body: JsonBody<UpdateCustomerRequest>,
) -> Result<Json<CustomerResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let input = UpdateCustomer {
        email: req.email,
        first_name: req.first_name,
        last_name: req.last_name,
        phone: req.phone,
    };
    let customer = update_customer(&ctx, ports.customers(), id, input).await?;
    Ok(Json(CustomerResponse::from(&customer)))
}

pub(super) async fn add_address_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<CustomerId>,
    body: JsonBody<AddressRequest>,
) -> Result<(StatusCode, Json<CustomerResponse>), ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let customer = add_address(&ctx, ports.customers(), &state.ids, id, req.into()).await?;
    Ok((StatusCode::CREATED, Json(CustomerResponse::from(&customer))))
}

pub(super) async fn update_address_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId((id, address)): PathId<(CustomerId, AddressId)>,
    body: JsonBody<AddressRequest>,
) -> Result<Json<CustomerResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let customer = update_address(&ctx, ports.customers(), id, address, req.into()).await?;
    Ok(Json(CustomerResponse::from(&customer)))
}

pub(super) async fn remove_address_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId((id, address)): PathId<(CustomerId, AddressId)>,
) -> Result<Json<CustomerResponse>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let customer = remove_address(&ctx, ports.customers(), id, address).await?;
    Ok(Json(CustomerResponse::from(&customer)))
}
