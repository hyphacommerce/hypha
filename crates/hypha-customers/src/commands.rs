use hypha_core::{
    Actor, Address, AddressError, AddressId, AddressParts, AuthError, Clock, Ctx, CustomerGroupId,
    CustomerId, Email, EmailError, ErrorCode, Handle, IdGenerator, Keyset, Page, PageRequest,
    RepoError, Scope, report_duplicate,
};
use thiserror::Error;

use crate::{
    Customer, CustomerError, CustomerFilter, CustomerSort, CustomerSummary,
    consent::{Channel, ConsentChange, ConsentSource},
    repo::{CustomerRepo, keys},
};

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum CommandError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Repo(#[from] RepoError),
    #[error(transparent)]
    Customer(#[from] CustomerError),
    #[error(transparent)]
    Email(#[from] EmailError),
    #[error(transparent)]
    Address(#[from] AddressError),
    #[error("customer {0} not found")]
    CustomerNotFound(CustomerId),
    #[error("{email} is already customer {customer}")]
    CustomerExists { email: Email, customer: CustomerId },
    #[error("customer group {0} not found")]
    GroupNotFound(CustomerGroupId),
    #[error("a customer group with the handle {0} exists")]
    GroupHandleTaken(Handle),
    #[error("a customer group named {0:?} exists")]
    GroupNameTaken(String),
}

impl ErrorCode for CommandError {
    fn code(&self) -> &'static str {
        match self {
            CommandError::Auth(e) => e.code(),
            CommandError::Repo(e) => e.code(),
            CommandError::Customer(e) => e.code(),
            CommandError::Email(e) => e.code(),
            CommandError::Address(e) => e.code(),
            CommandError::CustomerNotFound(_) => "customer_not_found",
            CommandError::CustomerExists { .. } => "customer_exists",
            CommandError::GroupNotFound(_) => "customer_group_not_found",
            CommandError::GroupHandleTaken(_) => "customer_group_handle_taken",
            CommandError::GroupNameTaken(_) => "customer_group_name_taken",
        }
    }
}

pub struct CreateCustomer {
    pub email: String,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub phone: Option<String>,
}

pub async fn create_customer<R: CustomerRepo, I: IdGenerator>(
    ctx: &Ctx,
    customers: &R,
    ids: &I,
    input: CreateCustomer,
) -> Result<Customer, CommandError> {
    ctx.require(Scope::CustomersWrite)?;

    let email = Email::new(&input.email)?;
    let mut customer = Customer::new(CustomerId::from_uuid(ids.new_uuid()), email.clone());
    customer.set_first_name(input.first_name)?;
    customer.set_last_name(input.last_name)?;
    customer.set_phone(input.phone)?;

    if let Some(taken) = email_taken(customers, &email, customer.id()).await? {
        return Err(taken);
    }
    let written = customers.insert(&mut customer).await;
    let race = email_taken(customers, &email, customer.id());
    report_duplicate(written, keys::CUSTOMER_EMAIL, race).await?;
    Ok(customer)
}

/// The customer with `email`, created (with no names) if there is none yet. How checkout
/// turns a guest into a customer.
pub async fn find_or_create_customer<R: CustomerRepo, I: IdGenerator>(
    ctx: &Ctx,
    customers: &R,
    ids: &I,
    email: &Email,
) -> Result<Customer, CommandError> {
    ctx.require(Scope::CustomersWrite)?;

    if let Some(existing) = customers.get_by_email(email).await? {
        return Ok(existing);
    }
    let mut customer = Customer::new(CustomerId::from_uuid(ids.new_uuid()), email.clone());
    match customers.insert(&mut customer).await {
        Ok(()) => Ok(customer),
        // Another checkout created them first: theirs is the customer.
        Err(RepoError::Duplicate(keys::CUSTOMER_EMAIL)) => customers
            .get_by_email(email)
            .await?
            .ok_or(RepoError::Duplicate(keys::CUSTOMER_EMAIL).into()),
        Err(e) => Err(e.into()),
    }
}

pub async fn get_customer<R: CustomerRepo>(
    ctx: &Ctx,
    customers: &R,
    id: CustomerId,
) -> Result<Customer, CommandError> {
    ctx.require(Scope::CustomersRead)?;
    load(customers, id).await
}

/// A page of customer summaries in `sort`'s order, narrowed to text `q` (email, name,
/// phone) when given and to `fields`, filters on custom fields checked by
/// `hypha-custom-fields` already. `page.after`'s key must fit the sort
/// (`CustomerSort::fits`; hosts check cursors).
pub async fn list_customers<R: CustomerRepo>(
    ctx: &Ctx,
    customers: &R,
    q: Option<String>,
    fields: Vec<hypha_core::FieldFilter>,
    group: Option<CustomerGroupId>,
    sort: CustomerSort,
    page: PageRequest<Keyset<CustomerId>>,
) -> Result<Page<CustomerSummary>, CommandError> {
    ctx.require(Scope::CustomersRead)?;
    let filter = CustomerFilter::new(q)?.with_fields(fields).in_group(group);
    Ok(customers.list(&filter, sort, page).await?)
}

/// Changes only the fields that are `Some`, like a JSON Merge Patch: `Some(None)` clears
/// an optional field (so does a blank string, which the setters trim away). The email
/// can be changed but not cleared.
#[derive(Default)]
pub struct UpdateCustomer {
    pub email: Option<String>,
    pub first_name: Option<Option<String>>,
    pub last_name: Option<Option<String>>,
    pub phone: Option<Option<String>>,
}

pub async fn update_customer<R: CustomerRepo>(
    ctx: &Ctx,
    customers: &R,
    id: CustomerId,
    input: UpdateCustomer,
) -> Result<Customer, CommandError> {
    ctx.require(Scope::CustomersWrite)?;

    let mut customer = load(customers, id).await?;
    if let Some(first_name) = input.first_name {
        customer.set_first_name(first_name)?;
    }
    if let Some(last_name) = input.last_name {
        customer.set_last_name(last_name)?;
    }
    if let Some(phone) = input.phone {
        customer.set_phone(phone)?;
    }
    let Some(email) = input.email else {
        customers.update(&mut customer).await?;
        return Ok(customer);
    };

    let email = Email::new(&email)?;
    if let Some(taken) = email_taken(customers, &email, id).await? {
        return Err(taken);
    }
    customer.set_email(email.clone())?;
    let written = customers.update(&mut customer).await;
    report_duplicate(
        written,
        keys::CUSTOMER_EMAIL,
        email_taken(customers, &email, id),
    )
    .await?;
    Ok(customer)
}

/// Where a consent written through the API comes from: an app is itself, anyone else on the
/// admin surface is `admin`.
fn source_of(ctx: &Ctx) -> ConsentSource {
    match ctx.actor() {
        Actor::App(id) => ConsentSource::App(id),
        Actor::Customer(_) => ConsentSource::Storefront,
        Actor::System => ConsentSource::System,
        Actor::User(_) | Actor::Agent(_) | Actor::ApiKey(_) => ConsentSource::Admin,
    }
}

/// Records a customer's marketing consent for `channel`: the newest `consented_at` wins, so
/// a write older than what's stored changes nothing and answers `applied: false` with the
/// customer as it is. The source is the caller's, never input.
pub async fn set_marketing_consent<R: CustomerRepo>(
    ctx: &Ctx,
    customers: &R,
    clock: &impl Clock,
    id: CustomerId,
    channel: Channel,
    change: ConsentChange,
) -> Result<(Customer, bool), CommandError> {
    ctx.require(Scope::CustomersWrite)?;

    let mut customer = load(customers, id).await?;
    let applied = customer.set_marketing_consent(channel, change, source_of(ctx), clock.now())?;
    if applied {
        customers.update(&mut customer).await?;
    }
    Ok((customer, applied))
}

/// An address for the address book, and whether it becomes a default.
pub struct SaveAddress {
    pub address: AddressParts,
    pub default_shipping: bool,
    pub default_billing: bool,
}

pub async fn add_address<R: CustomerRepo, I: IdGenerator>(
    ctx: &Ctx,
    customers: &R,
    ids: &I,
    customer: CustomerId,
    input: SaveAddress,
) -> Result<Customer, CommandError> {
    ctx.require(Scope::CustomersWrite)?;

    let address = Address::new(input.address)?;
    let mut customer = load(customers, customer).await?;
    customer.add_address(
        AddressId::from_uuid(ids.new_uuid()),
        address,
        input.default_shipping,
        input.default_billing,
    )?;
    customers.update(&mut customer).await?;
    Ok(customer)
}

/// Replaces an address, defaults included: `false` stops it being that default.
pub async fn update_address<R: CustomerRepo>(
    ctx: &Ctx,
    customers: &R,
    customer: CustomerId,
    address: AddressId,
    input: SaveAddress,
) -> Result<Customer, CommandError> {
    ctx.require(Scope::CustomersWrite)?;

    let parts = Address::new(input.address)?;
    let mut customer = load(customers, customer).await?;
    customer.update_address(
        address,
        parts,
        input.default_shipping,
        input.default_billing,
    )?;
    customers.update(&mut customer).await?;
    Ok(customer)
}

pub async fn remove_address<R: CustomerRepo>(
    ctx: &Ctx,
    customers: &R,
    customer: CustomerId,
    address: AddressId,
) -> Result<Customer, CommandError> {
    ctx.require(Scope::CustomersWrite)?;

    let mut customer = load(customers, customer).await?;
    customer.remove_address(address)?;
    customers.update(&mut customer).await?;
    Ok(customer)
}

async fn load<R: CustomerRepo>(customers: &R, id: CustomerId) -> Result<Customer, CommandError> {
    customers
        .get(id)
        .await?
        .ok_or(CommandError::CustomerNotFound(id))
}

/// `CustomerExists` naming the customer other than `except` who has `email`, if any.
async fn email_taken<R: CustomerRepo>(
    customers: &R,
    email: &Email,
    except: CustomerId,
) -> Result<Option<CommandError>, CommandError> {
    Ok(customers
        .get_by_email(email)
        .await?
        .filter(|existing| existing.id() != except)
        .map(|existing| CommandError::CustomerExists {
            email: email.clone(),
            customer: existing.id(),
        }))
}
