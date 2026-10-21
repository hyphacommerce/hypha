//! Customers and their address books (all live in `hypha-customers`).

use hypha_core::Stamped;

use super::stamp_times;
use axum::{Json, extract::State, http::StatusCode};
use hypha_core::{
    AddressId, AddressParts, CountryCode, CustomerId, Email, Keyset, Money, Scope, SortKey,
};
use hypha_customers::{
    CreateCustomer, Customer, CustomerSort, CustomerSummary, HasCustomers, SaveAddress,
    SavedAddress, UpdateCustomer, add_address, create_customer, get_customer, list_customers,
    remove_address, update_address, update_customer,
};
use serde::{Deserialize, Serialize};

use hypha_apps::HasApps;
use hypha_core::Ctx;
use hypha_custom_fields::{HasCustomFieldDefinitions, HasCustomFieldValues, OwnerType};
use hypha_orders::{HasOrders, OrderStats, customer_order_stats};

use super::{
    custom_fields::{self, CustomFieldValues, FieldParams},
    page::{ListQuery, PageParams, PageResponse, SortedFilters},
};
use crate::{
    error::ApiError,
    extract::{Caller, JsonBody, PathId},
    patch,
    redact::{self, Family, Redacted},
    state::{SharedState, Storage},
};

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateCustomerRequest {
    email: String,
    first_name: Option<String>,
    last_name: Option<String>,
    phone: Option<String>,
}

/// JSON Merge Patch (see `patch`): only the fields sent change, `null` clears a name or
/// phone. The email can be changed but not cleared.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateCustomerRequest {
    #[serde(default, deserialize_with = "patch::non_null")]
    // Optional, but `null` is refused (`patch::non_null`): not nullable in the spec.
    #[schema(value_type = String, required = false)]
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
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AddressRequest {
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

#[derive(Serialize, utoipa::ToSchema)]
pub struct CustomerResponse {
    id: CustomerId,
    email: Email,
    first_name: Option<String>,
    last_name: Option<String>,
    phone: Option<String>,
    default_shipping_address: Option<AddressId>,
    default_billing_address: Option<AddressId>,
    addresses: Vec<AddressResponse>,
    /// Its custom field values the caller may read, `{"custom.tier": "gold", …}`
    /// (design note 0033); `PATCH /admin/customers/{id}/custom-fields` writes them.
    #[schema(value_type = std::collections::HashMap<String, serde_json::Value>)]
    custom_fields: CustomFieldValues,
    /// What the customer's orders add up to (design note 0032).
    orders: OrderStatsResponse,
    /// When it was created, RFC 3339 in UTC.
    created_at: String,
    /// When it last changed, RFC 3339 in UTC.
    updated_at: String,
    /// Personal fields left out for this caller (design note 0029, AP10): an API key or
    /// app without the matching `customer_data:*` scope (`custom_fields.<key>` for a
    /// personal custom field, without `customer_data:profile`). They are absent from the
    /// object then, whatever the schema says. Never sent to staff or storefronts.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    redacted: Option<Vec<String>>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct AddressResponse {
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
    /// Personal fields left out for this caller (design note 0029, AP10): an API key or
    /// app without the matching `customer_data:*` scope. They are absent from the
    /// object then, whatever the schema says. Never sent to staff or storefronts.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    redacted: Option<Vec<String>>,
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
            redacted: None,
        }
    }
}

/// A customer's orders, added up over those that weren't cancelled (design note 0032,
/// §8): `GET /admin/orders?customer=` lists them.
#[derive(Serialize, utoipa::ToSchema)]
pub struct OrderStatsResponse {
    /// How many orders the customer placed that weren't cancelled.
    count: u64,
    /// When the first of them was placed, RFC 3339 in UTC; absent without orders.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    first_placed_at: Option<String>,
    /// When the last of them was placed, RFC 3339 in UTC; absent without orders.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    last_placed_at: Option<String>,
    /// Their totals, one per currency the customer paid in, by currency code.
    amount_spent: Vec<Money>,
}

impl OrderStatsResponse {
    fn new(s: OrderStats) -> Result<Self, ApiError> {
        Ok(Self {
            count: s.orders_count,
            first_placed_at: s.first_order_at.map(super::rfc3339).transpose()?,
            last_placed_at: s.last_order_at.map(super::rfc3339).transpose()?,
            amount_spent: s.amount_spent,
        })
    }
}

impl CustomerResponse {
    fn new(
        c: &Customer,
        custom_fields: CustomFieldValues,
        orders: OrderStats,
    ) -> Result<Self, ApiError> {
        let (created_at, updated_at) = stamp_times(c.stamps())?;
        Ok(Self {
            created_at,
            updated_at,
            orders: OrderStatsResponse::new(orders)?,
            id: c.id(),
            email: c.email().clone(),
            first_name: c.first_name().map(str::to_string),
            last_name: c.last_name().map(str::to_string),
            phone: c.phone().map(str::to_string),
            default_shipping_address: c.default_shipping(),
            default_billing_address: c.default_billing(),
            addresses: c.addresses().iter().map(AddressResponse::from).collect(),
            custom_fields,
            redacted: None,
        })
    }
}

/// The admin's response for `customer`: with its custom fields, as `ctx` may see it.
async fn respond<P>(ctx: &Ctx, ports: &P, customer: &Customer) -> Result<Redacted, ApiError>
where
    P: HasApps + HasCustomFieldDefinitions + HasCustomFieldValues + HasOrders,
{
    let kind = custom_fields::caller_kind(ctx, ports).await?;
    let id = customer.id().as_uuid();
    let fields =
        custom_fields::read(ctx, &kind, ports, &[OwnerType::Customer], &[id], None).await?;
    let stats = customer_order_stats(ctx, ports.orders(), customer.id()).await?;
    let dto = CustomerResponse::new(customer, fields.json_of(id), stats)?;
    redact::one_with_fields(ctx, Family::Customer, dto, &fields.redacted)
}

/// A customer in a list: without the address book (design note 0024).
#[derive(Serialize, utoipa::ToSchema)]
pub struct CustomerSummaryResponse {
    id: CustomerId,
    email: Email,
    first_name: Option<String>,
    last_name: Option<String>,
    phone: Option<String>,
    /// How many addresses the address book holds.
    address_count: u64,
    /// When the customer was created, RFC 3339 in UTC.
    created_at: String,
    /// When the customer last changed, RFC 3339 in UTC.
    updated_at: String,
    /// The custom field values `?custom_fields=` asked for (design note 0033); absent
    /// without it.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<std::collections::HashMap<String, serde_json::Value>>, required = false)]
    custom_fields: Option<CustomFieldValues>,
    /// Personal fields left out for this caller (design note 0029, AP10): an API key or
    /// app without the matching `customer_data:*` scope. They are absent from the
    /// object then, whatever the schema says. Never sent to staff or storefronts.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    redacted: Option<Vec<String>>,
}

impl CustomerSummaryResponse {
    fn new(c: CustomerSummary, custom_fields: Option<CustomFieldValues>) -> Result<Self, ApiError> {
        let (created_at, updated_at) = stamp_times(c.stamps)?;
        Ok(Self {
            id: c.id,
            email: c.email,
            first_name: c.first_name,
            last_name: c.last_name,
            phone: c.phone,
            address_count: c.address_count as u64,
            created_at,
            updated_at,
            custom_fields,
            redacted: None,
        })
    }
}

/// A page of customers (named for the documents: utoipa's `body =` takes a plain name).
type CustomerSummaryPage = PageResponse<CustomerSummaryResponse>;

#[derive(Deserialize, utoipa::IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct CustomerFilters {
    /// Only customers whose email, first or last name or phone contains this text,
    /// ignoring case. Up to 255 characters; blank filters nothing. A caller that isn't
    /// a staff session needs `customer_data:name`, `:email` and `:phone` for it
    /// (sorting by email or name, the matching one).
    q: Option<String>,
    #[param(inline)]
    sort: Option<CustomerSortDto>,
}

/// The order of a customer list (design note 0024, AU1). Default `newest`. A name is
/// the first name, a space and the last name, compared lowercased character by
/// character; ties go by creation, in the same direction.
#[derive(Debug, Clone, Copy, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CustomerSortDto {
    Newest,
    Oldest,
    EmailAsc,
    EmailDesc,
    NameAsc,
    NameDesc,
}

impl CustomerFilters {
    fn sort(&self) -> CustomerSort {
        match self.sort {
            None | Some(CustomerSortDto::Newest) => CustomerSort::Newest,
            Some(CustomerSortDto::Oldest) => CustomerSort::Oldest,
            Some(CustomerSortDto::EmailAsc) => CustomerSort::EmailAsc,
            Some(CustomerSortDto::EmailDesc) => CustomerSort::EmailDesc,
            Some(CustomerSortDto::NameAsc) => CustomerSort::NameAsc,
            Some(CustomerSortDto::NameDesc) => CustomerSort::NameDesc,
        }
    }
}

impl SortedFilters for CustomerFilters {
    fn fits(&self, key: &SortKey) -> bool {
        self.sort().fits(key)
    }
}

/// Customers, newest first or in `sort`'s order, optionally matching text and custom
/// field filters.
#[utoipa::path(
    get,
    path = "/admin/customers",
    params(PageParams, CustomerFilters, FieldParams),
    responses((status = 200, description = "OK", body = CustomerSummaryPage)),
    extensions(("x-hypha-scopes" = json!(["customers:read"]))),
)]
pub async fn list<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    query: ListQuery<CustomerFilters>,
) -> Result<Json<Redacted>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let (page, filters, scope, field_query) = query.parse_sorted_with_fields()?;
    let fields =
        custom_fields::list_fields(&ctx, &ports, OwnerType::Customer, &field_query).await?;
    let sort = filters.sort();
    // Searching or sorting by a personal field tells what it holds: only for callers
    // who may see it (design note 0029, AP10).
    if filters.q.as_deref().is_some_and(|q| !q.trim().is_empty()) {
        redact::require_fields(
            &ctx,
            &[
                Scope::CustomerDataName,
                Scope::CustomerDataEmail,
                Scope::CustomerDataPhone,
            ],
        )?;
    }
    match sort {
        CustomerSort::EmailAsc | CustomerSort::EmailDesc => {
            redact::require_fields(&ctx, &[Scope::CustomerDataEmail])?
        }
        CustomerSort::NameAsc | CustomerSort::NameDesc => {
            redact::require_fields(&ctx, &[Scope::CustomerDataName])?
        }
        CustomerSort::Newest | CustomerSort::Oldest => {}
    }
    let page = list_customers(
        &ctx,
        ports.customers(),
        filters.q,
        fields.filters,
        sort,
        page,
    )
    .await?;
    let values = match &fields.keys {
        Some(keys) => {
            let owners: Vec<uuid::Uuid> = page.items.iter().map(|c| c.id.as_uuid()).collect();
            Some(
                custom_fields::read(
                    &ctx,
                    &fields.kind,
                    &ports,
                    &[OwnerType::Customer],
                    &owners,
                    Some(keys),
                )
                .await?,
            )
        }
        None => None,
    };
    let items = page
        .items
        .iter()
        .cloned()
        .map(|c| {
            let custom = values.as_ref().map(|v| v.json_of(c.id.as_uuid()));
            CustomerSummaryResponse::new(c, custom)
        })
        .collect::<Result<_, _>>()?;
    let page = PageResponse::sorted(
        &page,
        &scope,
        |c| Keyset {
            key: sort.key(c),
            id: c.id,
        },
        items,
    )?;
    let hidden = values.map(|v| v.redacted).unwrap_or_default();
    Ok(Json(redact::page_with_fields(
        &ctx,
        Family::Customer,
        page,
        &hidden,
    )?))
}

/// Creates a customer.
#[utoipa::path(
    post,
    path = "/admin/customers",
    request_body = CreateCustomerRequest,
    responses((status = 201, description = "Created", body = CustomerResponse)),
    extensions(("x-hypha-scopes" = json!(["customers:write"]))),
)]
pub async fn create<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    body: JsonBody<CreateCustomerRequest>,
) -> Result<(StatusCode, Json<Redacted>), ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let input = CreateCustomer {
        email: req.email,
        first_name: req.first_name,
        last_name: req.last_name,
        phone: req.phone,
    };
    let customer = create_customer(&ctx, ports.customers(), &state.ids, input).await?;
    Ok((
        StatusCode::CREATED,
        Json(respond(&ctx, &ports, &customer).await?),
    ))
}

/// A customer with their address book.
#[utoipa::path(
    get,
    path = "/admin/customers/{id}",
    params(("id" = CustomerId, Path)),
    responses((status = 200, description = "OK", body = CustomerResponse)),
    extensions(("x-hypha-scopes" = json!(["customers:read"]))),
)]
pub async fn fetch<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<CustomerId>,
) -> Result<Json<Redacted>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let customer = get_customer(&ctx, ports.customers(), id).await?;
    Ok(Json(respond(&ctx, &ports, &customer).await?))
}

/// Changes a customer's details (JSON Merge Patch).
#[utoipa::path(
    patch,
    path = "/admin/customers/{id}",
    params(("id" = CustomerId, Path)),
    request_body = UpdateCustomerRequest,
    responses((status = 200, description = "OK", body = CustomerResponse)),
    extensions(("x-hypha-scopes" = json!(["customers:write"]))),
)]
pub async fn update<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<CustomerId>,
    body: JsonBody<UpdateCustomerRequest>,
) -> Result<Json<Redacted>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let input = UpdateCustomer {
        email: req.email,
        first_name: req.first_name,
        last_name: req.last_name,
        phone: req.phone,
    };
    let customer = update_customer(&ctx, ports.customers(), id, input).await?;
    Ok(Json(respond(&ctx, &ports, &customer).await?))
}

/// Adds an address to a customer's address book.
#[utoipa::path(
    post,
    path = "/admin/customers/{id}/addresses",
    params(("id" = CustomerId, Path)),
    request_body = AddressRequest,
    responses((status = 201, description = "Created", body = CustomerResponse)),
    extensions(("x-hypha-scopes" = json!(["customers:write"]))),
)]
pub async fn add_address_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId(id): PathId<CustomerId>,
    body: JsonBody<AddressRequest>,
) -> Result<(StatusCode, Json<Redacted>), ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let customer = add_address(&ctx, ports.customers(), &state.ids, id, req.into()).await?;
    Ok((
        StatusCode::CREATED,
        Json(respond(&ctx, &ports, &customer).await?),
    ))
}

/// Replaces a saved address.
#[utoipa::path(
    put,
    path = "/admin/customers/{id}/addresses/{address_id}",
    params(("id" = CustomerId, Path), ("address_id" = AddressId, Path)),
    request_body = AddressRequest,
    responses((status = 200, description = "OK", body = CustomerResponse)),
    extensions(("x-hypha-scopes" = json!(["customers:write"]))),
)]
pub async fn update_address_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId((id, address)): PathId<(CustomerId, AddressId)>,
    body: JsonBody<AddressRequest>,
) -> Result<Json<Redacted>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let req = body.parse()?;
    let customer = update_address(&ctx, ports.customers(), id, address, req.into()).await?;
    Ok(Json(respond(&ctx, &ports, &customer).await?))
}

/// Removes a saved address.
#[utoipa::path(
    delete,
    path = "/admin/customers/{id}/addresses/{address_id}",
    params(("id" = CustomerId, Path), ("address_id" = AddressId, Path)),
    responses((status = 200, description = "OK", body = CustomerResponse)),
    extensions(("x-hypha-scopes" = json!(["customers:write"]))),
)]
pub async fn remove_address_handler<S: Storage>(
    State(state): State<SharedState<S>>,
    caller: Caller,
    PathId((id, address)): PathId<(CustomerId, AddressId)>,
) -> Result<Json<Redacted>, ApiError> {
    let (ports, ctx) = caller.authenticate(&state).await?;
    let customer = remove_address(&ctx, ports.customers(), id, address).await?;
    Ok(Json(respond(&ctx, &ports, &customer).await?))
}
