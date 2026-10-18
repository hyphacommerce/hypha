//! `Mailer` (design note 0028, §4, NT7): the event handler that emails buyers and staff
//! about orders, with the dispatcher's retries and lease.

use hypha_core::{Email, EventHandler, EventId, FulfillmentId, OrderId, StoredEvent, Timezone};
use hypha_fulfillment::{Fulfillment, FulfillmentRepo, HasFulfillments};
use hypha_media::HasAssets;
use hypha_notifications::{
    HasNotificationSettings, HasNotificationTemplates, HasNotifications, Notification, TemplateKey,
};
use hypha_orders::{HasOrders, Order, OrderRepo};
use hypha_payments::{HasPayments, newest_payment};
use hypha_stores::StoreRepo;
use serde::Deserialize;

use crate::{
    CommandError, MailEnv,
    context::{
        CancellationContext, CancellationMail, CustomerContext, FulfillmentContext, MailContext,
        NewOrderMail, OrderContext, OrderMail, ShipmentMail,
    },
    send::{Branding, Message, branding, deliver},
};

/// Every repository sending reads or writes, so a host names one bound. The store's
/// row is read through `StoreRepo` on the same ports: one store per deployment (design
/// note 0006, model A) keeps it in the store's database.
pub trait MailPorts:
    HasOrders
    + HasFulfillments
    + HasPayments
    + HasAssets
    + HasNotificationTemplates
    + HasNotificationSettings
    + HasNotifications
    + StoreRepo
{
}

impl<T> MailPorts for T where
    T: HasOrders
        + HasFulfillments
        + HasPayments
        + HasAssets
        + HasNotificationTemplates
        + HasNotificationSettings
        + HasNotifications
        + StoreRepo
        + ?Sized
{
}

/// The event types the mailer answers.
pub const ORDER_PLACED: &str = "order.placed";
pub const ORDER_CANCELLED: &str = "order.cancelled";
pub const FULFILLMENT_CREATED: &str = "fulfillment.created";
pub const FULFILLMENT_UPDATED: &str = "fulfillment.updated";

/// Emails about orders, on their events (design note 0028, §4):
/// - `order.placed`: `order_confirmation` to the buyer, `staff_new_order` to each staff
///   recipient;
/// - `order.cancelled`, `fulfillment.created`, `fulfillment.updated`: `order_cancelled`,
///   `order_shipped`, `order_tracking_updated` to the buyer, when the event's
///   `notify_customer` says so (events recorded before it existed don't).
///
/// It holds the host's `MailEnv` (the sender, links, the store), so a host builds its
/// handler set at startup rather than as a `const`. Each message is recorded once per
/// (event, template, recipient): a run that finds one sent (or skipped) sends nothing
/// more, so running twice is harmless. Messages go one after another; a transient
/// failure stops the run with `Err`, and the dispatcher's retry sends the rest.
pub struct Mailer<E> {
    env: E,
}

impl<E> Mailer<E> {
    /// Deliveries to it are `handler:mailer`: a contract, never renamed.
    pub const NAME: &str = "mailer";

    pub fn new(env: E) -> Self {
        Self { env }
    }

    pub fn env(&self) -> &E {
        &self.env
    }
}

impl<P, E> EventHandler<P> for Mailer<E>
where
    P: MailPorts,
    E: MailEnv,
{
    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn wants(&self, event_type: &str) -> bool {
        [
            ORDER_PLACED,
            ORDER_CANCELLED,
            FULFILLMENT_CREATED,
            FULFILLMENT_UPDATED,
        ]
        .contains(&event_type)
    }

    async fn handle(&self, ports: &P, event: &StoredEvent) -> Result<(), String> {
        mail_event(&self.env, ports, event)
            .await
            .map(drop)
            .map_err(|e| e.to_string())
    }
}

/// What the mailer reads from the four events' payloads.
#[derive(Deserialize)]
struct Payload {
    order: OrderId,
    fulfillment: Option<FulfillmentId>,
    #[serde(default)]
    notify_customer: bool,
}

/// Sends what `event` calls for and returns the records, in order. An event the mailer
/// doesn't answer, or one staff chose not to tell the buyer about, sends nothing.
pub async fn mail_event<E: MailEnv, P: MailPorts>(
    env: &E,
    ports: &P,
    event: &StoredEvent,
) -> Result<Vec<Notification>, CommandError> {
    let messages = messages_for(env, ports, event).await?;
    let Some((branding, messages)) = messages else {
        return Ok(Vec::new());
    };
    let mut sent = Vec::with_capacity(messages.len());
    for message in messages {
        sent.push(deliver(env, ports, &branding, message).await?);
    }
    Ok(sent)
}

/// The messages `event` calls for, with the branding they were built with; `None` when
/// it calls for none.
pub(crate) async fn messages_for<E: MailEnv, P: MailPorts>(
    env: &E,
    ports: &P,
    event: &StoredEvent,
) -> Result<Option<(Branding, Vec<Message>)>, CommandError> {
    let template = match event.event_type.as_str() {
        ORDER_PLACED => TemplateKey::OrderConfirmation,
        ORDER_CANCELLED => TemplateKey::OrderCancelled,
        FULFILLMENT_CREATED => TemplateKey::OrderShipped,
        FULFILLMENT_UPDATED => TemplateKey::OrderTrackingUpdated,
        _ => return Ok(None),
    };
    let payload: Payload =
        serde_json::from_value(event.data.clone()).map_err(|e| CommandError::BadEvent {
            event: EventId::from_uuid(event.id),
            event_type: event.event_type.clone(),
            reason: e.to_string(),
        })?;
    if template != TemplateKey::OrderConfirmation && !payload.notify_customer {
        return Ok(None);
    }
    let branding = branding(env, ports).await?;
    let order = load_order(ports, payload.order).await?;
    let fulfillment = match (template, payload.fulfillment) {
        (TemplateKey::OrderShipped | TemplateKey::OrderTrackingUpdated, Some(id)) => {
            Some(load_fulfillment(ports, id).await?)
        }
        _ => None,
    };
    let event_id = Some(EventId::from_uuid(event.id));
    let mut messages = Vec::new();
    let context = order_context(
        env,
        ports,
        &branding,
        template,
        &order,
        fulfillment.as_ref(),
    )
    .await?;
    messages.push(Message {
        template,
        to: order.email().clone(),
        event: event_id,
        order: Some(order.id()),
        context,
        even_if_disabled: false,
    });
    if template == TemplateKey::OrderConfirmation {
        let recipients: Vec<Email> = branding.settings.staff_recipients().to_vec();
        if !recipients.is_empty() {
            let context = order_context(
                env,
                ports,
                &branding,
                TemplateKey::StaffNewOrder,
                &order,
                None,
            )
            .await?;
            for to in recipients {
                messages.push(Message {
                    template: TemplateKey::StaffNewOrder,
                    to,
                    event: event_id,
                    order: Some(order.id()),
                    context: context.clone(),
                    even_if_disabled: false,
                });
            }
        }
    }
    Ok(Some((branding, messages)))
}

pub(crate) async fn load_order<P: HasOrders>(
    ports: &P,
    id: OrderId,
) -> Result<Order, CommandError> {
    ports
        .orders()
        .get(id)
        .await?
        .ok_or(CommandError::OrderNotFound(id))
}

pub(crate) async fn load_fulfillment<P: HasFulfillments>(
    ports: &P,
    id: FulfillmentId,
) -> Result<Fulfillment, CommandError> {
    ports
        .fulfillments()
        .get(id)
        .await?
        .ok_or(CommandError::FulfillmentNotFound(id))
}

/// The newest fulfilment of `order` that stands, else its newest: what a shipping email
/// without an event to say which is about.
pub(crate) async fn newest_fulfillment<P: HasFulfillments>(
    ports: &P,
    order: OrderId,
) -> Result<Fulfillment, CommandError> {
    let all = ports.fulfillments().list_for_order(order).await?;
    let standing = all.iter().position(Fulfillment::stands);
    all.into_iter()
        .nth(standing.unwrap_or(0))
        .ok_or(CommandError::NoFulfillment(order))
}

/// The context of `template` (an order template or `staff_new_order`) for `order` as it
/// is now; the shipping templates need `fulfillment` (the order's newest without one).
pub async fn order_context<E: MailEnv, P: MailPorts>(
    env: &E,
    ports: &P,
    branding: &Branding,
    template: TemplateKey,
    order: &Order,
    fulfillment: Option<&Fulfillment>,
) -> Result<MailContext, CommandError> {
    let zone: Option<Timezone> = branding.store.timezone();
    let payment = newest_payment(ports, order.id()).await?;
    let url = branding
        .settings
        .order_url()
        .map(|pattern| pattern.link(order.id(), &env.order_token(order.id())));
    let mail = OrderMail {
        shop: branding.shop.clone(),
        brand: branding.brand.clone(),
        order: OrderContext::of(order, payment.as_ref(), url, zone),
        customer: CustomerContext::of(order),
    };
    Ok(match template {
        TemplateKey::OrderShipped | TemplateKey::OrderTrackingUpdated => {
            let newest;
            let fulfillment = match fulfillment {
                Some(f) => f,
                None => {
                    newest = newest_fulfillment(ports, order.id()).await?;
                    &newest
                }
            };
            MailContext::Shipment(ShipmentMail {
                fulfillment: FulfillmentContext::of(fulfillment, order),
                mail,
            })
        }
        TemplateKey::OrderCancelled => MailContext::Cancellation(CancellationMail {
            cancellation: CancellationContext::of(order.cancelled().map(|c| c.reason)),
            mail,
        }),
        TemplateKey::StaffNewOrder => MailContext::NewOrder(NewOrderMail {
            admin_url: env
                .admin_url()
                .map(|base| format!("{base}/orders/{}", order.id())),
            mail,
        }),
        // The confirmation; the staff templates have no order, and callers don't ask.
        TemplateKey::OrderConfirmation
        | TemplateKey::Layout
        | TemplateKey::StaffInvite
        | TemplateKey::StaffPasswordReset => MailContext::Order(mail),
    })
}
