//! Customer group commands: the groups themselves, and who's in them, written as sets with
//! one event per group.

use std::collections::{BTreeSet, HashMap};

use hypha_core::{
    Ctx, CustomerGroupId, CustomerId, Handle, HasBatch, IdGenerator, Page, PageRequest,
    PendingEvent, RepoError, Scope, StageEvents,
};

use crate::{
    CommandError, CustomerError, CustomerGroup, CustomerGroupDeleted, CustomerGroupMembersAdded,
    CustomerGroupMembersRemoved, group_subject,
    repo::{
        CustomerGroupRepo, CustomerRepo, HasCustomerGroups, HasCustomers, StageCustomerGroups, keys,
    },
};

/// Everything the group commands read and write, named once.
pub trait GroupPorts:
    HasCustomers + HasCustomerGroups + HasBatch<Batch: StageCustomerGroups + StageEvents>
{
}

impl<T> GroupPorts for T where
    T: HasCustomers + HasCustomerGroups + HasBatch<Batch: StageCustomerGroups + StageEvents>
{
}

pub struct CreateCustomerGroup {
    pub name: String,
    pub handle: String,
    pub description: Option<String>,
}

/// Changes only what's `Some`, like a JSON Merge Patch: `description: Some(None)`
/// clears it.
#[derive(Default)]
pub struct UpdateCustomerGroup {
    pub name: Option<String>,
    pub handle: Option<String>,
    pub description: Option<Option<String>>,
}

async fn load_group<S: HasCustomerGroups>(
    ports: &S,
    id: CustomerGroupId,
) -> Result<CustomerGroup, CommandError> {
    ports
        .customer_groups()
        .get(id)
        .await?
        .ok_or(CommandError::GroupNotFound(id))
}

/// The group other than `except` that holds `handle` or `name`, as the error to answer.
async fn taken<S: HasCustomerGroups>(
    ports: &S,
    handle: &Handle,
    name: &str,
    except: Option<CustomerGroupId>,
) -> Result<Option<CommandError>, CommandError> {
    let groups = ports.customer_groups().list_all().await?;
    let other = groups.iter().filter(|g| Some(g.id()) != except);
    for group in other {
        if group.handle() == handle {
            return Ok(Some(CommandError::GroupHandleTaken(handle.clone())));
        }
        if group.name().eq_ignore_ascii_case(name) {
            return Ok(Some(CommandError::GroupNameTaken(name.to_string())));
        }
    }
    Ok(None)
}

/// A `Duplicate` from the write is the same error as the check before it (a race must
/// look like a normal duplicate).
fn duplicate_as(e: RepoError, handle: &Handle, name: &str) -> CommandError {
    match e {
        RepoError::Duplicate(keys::CUSTOMER_GROUP_HANDLE) => {
            CommandError::GroupHandleTaken(handle.clone())
        }
        RepoError::Duplicate(keys::CUSTOMER_GROUP_NAME) => {
            CommandError::GroupNameTaken(name.to_string())
        }
        other => other.into(),
    }
}

pub async fn create_group<S: HasCustomerGroups>(
    ctx: &Ctx,
    ports: &S,
    ids: &impl IdGenerator,
    input: CreateCustomerGroup,
) -> Result<CustomerGroup, CommandError> {
    ctx.require(Scope::CustomersWrite)?;
    let handle = Handle::new(&input.handle).map_err(CustomerError::from)?;
    let mut group = CustomerGroup::new(
        CustomerGroupId::from_uuid(ids.new_uuid()),
        &input.name,
        handle.clone(),
        input.description,
    )?;
    if let Some(taken) = taken(ports, &handle, group.name(), None).await? {
        return Err(taken);
    }
    let name = group.name().to_string();
    ports
        .customer_groups()
        .insert(&mut group)
        .await
        .map_err(|e| duplicate_as(e, &handle, &name))?;
    Ok(group)
}

pub async fn update_group<S: HasCustomerGroups>(
    ctx: &Ctx,
    ports: &S,
    id: CustomerGroupId,
    input: UpdateCustomerGroup,
) -> Result<CustomerGroup, CommandError> {
    ctx.require(Scope::CustomersWrite)?;
    let mut group = load_group(ports, id).await?;
    if let Some(name) = &input.name {
        group.rename(name)?;
    }
    if let Some(handle) = &input.handle {
        group.set_handle(Handle::new(handle).map_err(CustomerError::from)?);
    }
    if let Some(description) = input.description {
        group.set_description(description)?;
    }
    let (handle, name) = (group.handle().clone(), group.name().to_string());
    if let Some(taken) = taken(ports, &handle, &name, Some(id)).await? {
        return Err(taken);
    }
    ports
        .customer_groups()
        .update(&mut group)
        .await
        .map_err(|e| duplicate_as(e, &handle, &name))?;
    Ok(group)
}

/// Removes the group with its memberships, recording `customer_group.deleted` with the
/// customers that were in it. Once prices or payment methods point at groups, one in
/// use is refused (`RepoError::Reference`).
pub async fn delete_group<S: GroupPorts>(
    ctx: &Ctx,
    ports: &S,
    id: CustomerGroupId,
) -> Result<(), CommandError> {
    ctx.require(Scope::CustomersWrite)?;
    load_group(ports, id).await?;
    let customers = ports.customer_groups().members_in(id).await?;
    let mut batch = ports.batch();
    batch.delete_group(id);
    let event = CustomerGroupDeleted {
        group: id,
        customers,
    };
    batch.record_events(vec![PendingEvent::new(group_subject(id), &event)]);
    Ok(ports.commit(batch).await?)
}

pub async fn get_group<S: HasCustomerGroups>(
    ctx: &Ctx,
    ports: &S,
    id: CustomerGroupId,
) -> Result<CustomerGroup, CommandError> {
    ctx.require(Scope::CustomersRead)?;
    load_group(ports, id).await
}

/// Every group, by name.
pub async fn list_groups<S: HasCustomerGroups>(
    ctx: &Ctx,
    ports: &S,
) -> Result<Vec<CustomerGroup>, CommandError> {
    ctx.require(Scope::CustomersRead)?;
    Ok(ports.customer_groups().list_all().await?)
}

fn distinct(customers: Vec<CustomerId>) -> Vec<CustomerId> {
    customers
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Each of `customers` must exist (they arrive in a body: `customer_not_found` is 422).
async fn check_customers<S: HasCustomers>(
    ports: &S,
    customers: &[CustomerId],
) -> Result<(), CommandError> {
    for customer in customers {
        if ports.customers().get(*customer).await?.is_none() {
            return Err(CommandError::CustomerNotFound(*customer));
        }
    }
    Ok(())
}

/// Which of `customers` are in `group`.
async fn members<S: HasCustomerGroups>(
    ports: &S,
    group: CustomerGroupId,
    customers: &[CustomerId],
) -> Result<BTreeSet<CustomerId>, CommandError> {
    let groups = ports.customer_groups().groups_of(customers).await?;
    Ok(groups
        .into_iter()
        .filter(|(_, in_groups)| in_groups.contains(&group))
        .map(|(customer, _)| customer)
        .collect())
}

/// Adds customers to the group; ones already in it change nothing. Every customer
/// must exist.
pub async fn add_group_members<S: GroupPorts>(
    ctx: &Ctx,
    ports: &S,
    group: CustomerGroupId,
    customers: Vec<CustomerId>,
) -> Result<(), CommandError> {
    ctx.require(Scope::CustomersWrite)?;
    load_group(ports, group).await?;
    let customers = distinct(customers);
    check_customers(ports, &customers).await?;
    let already = members(ports, group, &customers).await?;
    let new: Vec<CustomerId> = customers
        .into_iter()
        .filter(|c| !already.contains(c))
        .collect();
    if new.is_empty() {
        return Ok(());
    }
    let mut batch = ports.batch();
    batch.add_group_members(group, &new);
    let event = CustomerGroupMembersAdded {
        group,
        customers: new,
    };
    batch.record_events(vec![PendingEvent::new(group_subject(group), &event)]);
    Ok(ports.commit(batch).await?)
}

/// Removes customers from the group; ones not in it change nothing.
pub async fn remove_group_members<S: GroupPorts>(
    ctx: &Ctx,
    ports: &S,
    group: CustomerGroupId,
    customers: Vec<CustomerId>,
) -> Result<(), CommandError> {
    ctx.require(Scope::CustomersWrite)?;
    load_group(ports, group).await?;
    let customers = distinct(customers);
    let present = members(ports, group, &customers).await?;
    let gone: Vec<CustomerId> = customers
        .into_iter()
        .filter(|c| present.contains(c))
        .collect();
    if gone.is_empty() {
        return Ok(());
    }
    let mut batch = ports.batch();
    batch.remove_group_members(group, &gone);
    let event = CustomerGroupMembersRemoved {
        group,
        customers: gone,
    };
    batch.record_events(vec![PendingEvent::new(group_subject(group), &event)]);
    Ok(ports.commit(batch).await?)
}

/// Makes `groups` the customer's groups (the customer page's card): adds it to the new
/// ones and removes it from the others, in one batch with an event per group. Every
/// group must exist (`group_not_found`, from the body). Answers the groups it's in.
pub async fn set_customer_groups<S: GroupPorts>(
    ctx: &Ctx,
    ports: &S,
    customer: CustomerId,
    groups: Vec<CustomerGroupId>,
) -> Result<Vec<CustomerGroupId>, CommandError> {
    ctx.require(Scope::CustomersWrite)?;
    if ports.customers().get(customer).await?.is_none() {
        return Err(CommandError::CustomerNotFound(customer));
    }
    let wanted: BTreeSet<CustomerGroupId> = groups.into_iter().collect();
    for group in &wanted {
        load_group(ports, *group).await?;
    }
    let current: BTreeSet<CustomerGroupId> = ports
        .customer_groups()
        .groups_of(&[customer])
        .await?
        .remove(&customer)
        .unwrap_or_default()
        .into_iter()
        .collect();
    let mut batch = ports.batch();
    let mut events = Vec::new();
    for group in wanted.difference(&current) {
        batch.add_group_members(*group, &[customer]);
        let event = CustomerGroupMembersAdded {
            group: *group,
            customers: vec![customer],
        };
        events.push(PendingEvent::new(group_subject(*group), &event));
    }
    for group in current.difference(&wanted) {
        batch.remove_group_members(*group, &[customer]);
        let event = CustomerGroupMembersRemoved {
            group: *group,
            customers: vec![customer],
        };
        events.push(PendingEvent::new(group_subject(*group), &event));
    }
    if events.is_empty() {
        return Ok(wanted.into_iter().collect());
    }
    batch.record_events(events);
    ports.commit(batch).await?;
    Ok(wanted.into_iter().collect())
}

/// Each customer's groups, sorted; a customer in none is absent.
pub async fn groups_of_customers<S: HasCustomerGroups>(
    ctx: &Ctx,
    ports: &S,
    customers: &[CustomerId],
) -> Result<HashMap<CustomerId, Vec<CustomerGroupId>>, CommandError> {
    ctx.require(Scope::CustomersRead)?;
    Ok(ports.customer_groups().groups_of(customers).await?)
}

/// A page of the group's customers, newest first.
pub async fn list_group_members<S: HasCustomerGroups>(
    ctx: &Ctx,
    ports: &S,
    group: CustomerGroupId,
    page: PageRequest<CustomerId>,
) -> Result<Page<CustomerId>, CommandError> {
    ctx.require(Scope::CustomersRead)?;
    load_group(ports, group).await?;
    Ok(ports.customer_groups().member_page(group, page).await?)
}
