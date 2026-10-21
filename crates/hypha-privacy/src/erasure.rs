//! Erasing a customer's personal data (design note 0032, §7.3) and exporting it (§7.4).

use hypha_auth::{
    HasIdentities, Identity, IdentityRepo, Principal, StageIdentities, StageSessions,
};
use hypha_cart::{CartRepo, HasCarts, StageCarts};
use hypha_core::{
    Clock, Ctx, CustomerId, Email, HasBatch, Keyset, OrderId, PageLimit, PageRequest, PendingEvent,
    Progress, RepoError, Scope, StageEvents, Timestamp,
};
use hypha_custom_fields::{
    DefinitionRepo, HasCustomFieldDefinitions, HasCustomFieldValues, OwnerType,
    StageCustomFieldValues, StoredValue, ValueRepo,
};
use hypha_customers::{
    Customer, CustomerDataRequested, CustomerRedacted, CustomerRepo, HasCustomers, StageCustomers,
    customer_subject,
};
use hypha_notifications::{HasNotifications, Notification, NotificationFilter, NotificationRepo};
use hypha_orders::{
    HasOrders, Order, OrderFilter, OrderFilterInput, OrderRepo, OrderSort, StageOrders,
};
use hypha_payments::{HasPayments, Payment, PaymentRepo, PaymentStatus};

use crate::{CommandError, PrivacySettings};

/// Everything erasure and export read and write, named once.
pub trait ErasurePorts:
    HasCustomers
    + HasOrders
    + HasCarts
    + HasPayments
    + HasNotifications
    + HasCustomFieldDefinitions
    + HasCustomFieldValues
    + HasIdentities
    + HasBatch<
        Batch: StageCustomers
                   + StageCarts
                   + StageOrders
                   + StageCustomFieldValues
                   + StageEvents
                   + StageIdentities
                   + StageSessions,
    >
{
}

impl<S> ErasurePorts for S where
    S: HasCustomers
        + HasOrders
        + HasCarts
        + HasPayments
        + HasNotifications
        + HasCustomFieldDefinitions
        + HasCustomFieldValues
        + HasIdentities
        + HasBatch<
            Batch: StageCustomers
                       + StageCarts
                       + StageOrders
                       + StageCustomFieldValues
                       + StageEvents
                       + StageIdentities
                       + StageSessions,
        >
{
}

/// The customer's account, if they have one (design note 0034): their identity, to go
/// with them (AC10).
async fn account_of<S: HasIdentities>(
    ports: &S,
    id: CustomerId,
) -> Result<Option<Identity>, RepoError> {
    ports
        .identities()
        .get_by_principal(Principal::Customer(id))
        .await
}

async fn load<S: HasCustomers>(ports: &S, id: CustomerId) -> Result<Customer, CommandError> {
    ports
        .customers()
        .get(id)
        .await?
        .ok_or(CommandError::CustomerNotFound(id))
}

/// Asks for the customer's personal data to be erased after `settings.erasure_delay`
/// (design note 0032, §7.3), recording `customer.erasure_requested`. Needs
/// `customers:erase`.
pub async fn request_erasure<S: HasCustomers>(
    ctx: &Ctx,
    ports: &S,
    clock: &impl Clock,
    settings: &PrivacySettings,
    id: CustomerId,
) -> Result<Customer, CommandError> {
    ctx.require(Scope::CustomersErase)?;
    let mut customer = load(ports, id).await?;
    customer.request_erasure(clock.now().plus(settings.erasure_delay))?;
    ports.customers().update(&mut customer).await?;
    Ok(customer)
}

/// Takes a request back within its window, recording `customer.erasure_cancelled`.
pub async fn cancel_erasure<S: HasCustomers>(
    ctx: &Ctx,
    ports: &S,
    id: CustomerId,
) -> Result<Customer, CommandError> {
    ctx.require(Scope::CustomersErase)?;
    let mut customer = load(ports, id).await?;
    customer.cancel_erasure()?;
    ports.customers().update(&mut customer).await?;
    Ok(customer)
}

/// Every order of `customer`, loaded whole.
async fn orders_of<S: HasOrders>(ports: &S, customer: CustomerId) -> Result<Vec<Order>, RepoError> {
    let filter = OrderFilter::new(OrderFilterInput {
        customer: Some(customer),
        ..OrderFilterInput::default()
    })
    .map_err(|e| RepoError::Backend(e.to_string()))?;
    let mut orders = Vec::new();
    let mut after: Option<Keyset<OrderId>> = None;
    loop {
        let page = ports
            .orders()
            .list(
                &filter,
                OrderSort::Newest,
                PageRequest {
                    after: after.clone(),
                    limit: PageLimit::new(PageLimit::MAX).unwrap_or_default(),
                },
            )
            .await?;
        for summary in &page.items {
            if let Some(order) = ports.orders().get(summary.id).await? {
                orders.push(order);
            }
        }
        match page.items.last() {
            Some(last) if page.has_more => {
                after = Some(Keyset {
                    key: OrderSort::Newest.key(last),
                    id: last.id,
                })
            }
            _ => return Ok(orders),
        }
    }
}

/// The personal custom field values the customer and its orders hold.
async fn personal_values<S: HasCustomFieldDefinitions + HasCustomFieldValues>(
    ports: &S,
    customer: CustomerId,
    orders: &[OrderId],
) -> Result<Vec<StoredValue>, RepoError> {
    let definitions = ports.custom_field_definitions().list(None).await?;
    let personal: Vec<_> = definitions
        .iter()
        .filter(|d| {
            d.personal() && matches!(d.owner_type(), OwnerType::Customer | OwnerType::Order)
        })
        .map(|d| d.id())
        .collect();
    if personal.is_empty() {
        return Ok(Vec::new());
    }
    let mut owners = vec![customer.as_uuid()];
    owners.extend(orders.iter().map(|o| o.as_uuid()));
    ports
        .custom_field_values()
        .get_many(&owners, Some(&personal))
        .await
}

/// What one erasure did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Erased {
    pub customer: CustomerId,
    pub orders: Vec<OrderId>,
}

/// Erases one customer's personal data now (design note 0032, §7.3), whether or not a
/// request was due: the job's unit of work, and what tests call. The customer, its
/// orders, its placed carts and its and its orders' personal custom field values are
/// written in one batch with `customer.redacted`; its open carts are deleted in it;
/// its email records are redacted just before (idempotently, so a failed batch leaves
/// nothing half done that a rerun wouldn't finish). An erased customer is left alone.
pub async fn erase_customer<S: ErasurePorts>(
    ports: &S,
    now: Timestamp,
    id: CustomerId,
) -> Result<Option<Erased>, CommandError> {
    let mut customer = load(ports, id).await?;
    if customer.is_erased() {
        return Ok(None);
    }
    let placeholder = Email::placeholder(id.as_uuid());
    let mut orders = orders_of(ports, id).await?;
    let order_ids: Vec<OrderId> = orders.iter().map(Order::id).collect();

    // Email records: by order, outside the batch (the notifications module has no
    // staged write; redacting twice changes nothing).
    for order in &order_ids {
        let mut after = None;
        loop {
            let page = ports
                .notifications()
                .list(
                    NotificationFilter {
                        order: Some(*order),
                        template: None,
                        status: None,
                    },
                    PageRequest {
                        after,
                        limit: PageLimit::new(PageLimit::MAX).unwrap_or_default(),
                    },
                )
                .await?;
            for mut record in page.items.iter().cloned() {
                if record.redact(placeholder.clone()) {
                    ports.notifications().update(&mut record).await?;
                }
            }
            match page.items.last() {
                Some(last) if page.has_more => after = Some(last.id()),
                _ => break,
            }
        }
    }

    let values = personal_values(ports, id, &order_ids).await?;
    let mut carts = ports.carts().of_customer(id).await?;
    // An open cart whose payment holds money (started online, never checked out;
    // design note 0031) isn't deleted: the payment points at it, and giving the money
    // back needs it. It's redacted like a placed one.
    let mut keep = Vec::new();
    for cart in &carts {
        if matches!(cart.status(), hypha_cart::CartStatus::Open) {
            let payments = ports.payments().list_for_cart(cart.id()).await?;
            if payments
                .iter()
                .any(|p| p.status().committed() || p.status() == PaymentStatus::Pending)
            {
                keep.push(cart.id());
            }
        }
    }
    customer.erase(now);
    let account = account_of(ports, id).await?;

    let mut batch = ports.batch();
    batch.update_customer(&mut customer);
    // Their account goes too (AC10): every session ends, the identity is removed.
    if let Some(identity) = &account {
        batch.revoke_sessions(identity.id(), None, now);
        batch.delete_identity(identity);
    }
    for order in &mut orders {
        if order.redact(placeholder.clone()) {
            batch.update_order(order);
        }
    }
    for cart in &mut carts {
        let open = matches!(cart.status(), hypha_cart::CartStatus::Open);
        if open && !keep.contains(&cart.id()) {
            batch.delete_cart(cart);
        } else if cart.redact(placeholder.clone()) {
            batch.update_cart(cart);
        }
    }
    for value in &values {
        batch.remove_custom_field_value(value.definition, value.owner, Some(value.version));
    }
    let event = CustomerRedacted {
        customer: id,
        orders: order_ids.clone(),
    };
    batch.record_events(vec![PendingEvent::new(customer_subject(id), &event)]);
    ports.commit(batch).await?;
    Ok(Some(Erased {
        customer: id,
        orders: order_ids,
    }))
}

/// What a run of the erasure step did: the erasures that went through, and each one
/// that didn't with why, so the host can log it (a customer whose erasure keeps failing
/// stays due and is tried every run).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Erasures {
    pub progress: Progress,
    pub failed: Vec<(CustomerId, CommandError)>,
}

/// The `erase_customers` step: runs every erasure due at `clock.now()`, up to `limit` a
/// run (design note 0032, §7.3). A customer whose erasure fails is left for the next
/// run and reported in `failed`; only the due list's own failure fails the step.
pub async fn erase_due<S: ErasurePorts>(
    ports: &S,
    clock: &impl Clock,
    limit: u32,
) -> Result<Erasures, CommandError> {
    let now = clock.now();
    let due = ports.customers().due_for_erasure(now, limit).await?;
    let mut done = 0;
    let mut failed = Vec::new();
    for id in &due {
        match erase_customer(ports, now, *id).await {
            Ok(_) => done += 1,
            Err(e) => failed.push((*id, e)),
        }
    }
    Ok(Erasures {
        progress: Progress::of(done, limit),
        failed,
    })
}

/// Everything Hypha holds about a customer (design note 0032, §7.4), for one JSON
/// document: the customer (its consent and custom fields come with it), its orders with
/// their payments, and its email records.
#[derive(Debug, Clone, PartialEq)]
pub struct CustomerExport {
    pub customer: Customer,
    pub orders: Vec<Order>,
    pub payments: Vec<Payment>,
    pub notifications: Vec<Notification>,
}

/// Answers a data request (design note 0032, §7.4), recording `customer.data_requested`
/// so apps can answer too. Needs `customers:read`; the route adds every `customer_data:*`
/// scope for callers that aren't staff.
pub async fn export_customer<S: ErasurePorts>(
    ctx: &Ctx,
    ports: &S,
    id: CustomerId,
) -> Result<CustomerExport, CommandError> {
    ctx.require(Scope::CustomersRead)?;
    let customer = load(ports, id).await?;
    let orders = orders_of(ports, id).await?;
    let mut payments = Vec::new();
    let mut notifications = Vec::new();
    for order in &orders {
        payments.extend(ports.payments().list_for_order(order.id()).await?);
        let mut after = None;
        loop {
            let page = ports
                .notifications()
                .list(
                    NotificationFilter {
                        order: Some(order.id()),
                        template: None,
                        status: None,
                    },
                    PageRequest {
                        after,
                        limit: PageLimit::new(PageLimit::MAX).unwrap_or_default(),
                    },
                )
                .await?;
            notifications.extend(page.items.iter().cloned());
            match page.items.last() {
                Some(last) if page.has_more => after = Some(last.id()),
                _ => break,
            }
        }
    }
    let mut batch = ports.batch();
    batch.record_events(vec![PendingEvent::new(
        customer_subject(id),
        &CustomerDataRequested { customer: id },
    )]);
    ports.commit(batch).await?;
    Ok(CustomerExport {
        customer,
        orders,
        payments,
        notifications,
    })
}
