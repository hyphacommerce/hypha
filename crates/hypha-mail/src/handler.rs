//! `Mailer` (design note 0028, §4, NT7): the event handler that queues emails to buyers
//! and staff about orders. It renders and records; the send job sends.

use hypha_core::{Email, EventHandler, EventId, FulfillmentId, OrderId, StoredEvent, Timezone};
use hypha_fulfillment::{Fulfillment, FulfillmentRepo, HasFulfillments};
use hypha_media::HasAssets;
use hypha_notifications::{
    HasNotificationSettings, HasNotificationTemplates, HasNotifications, Notification,
    NotificationRepo, TemplateKey,
};
use hypha_orders::{HasOrders, Order, OrderRepo};
use hypha_payments::{HasPayments, Payment, newest_payment};
use hypha_stores::StoreRepo;
use serde::Deserialize;

use crate::{
    CommandError, MailEnv,
    context::{
        CancellationContext, CancellationMail, CustomerContext, FulfillmentContext, MailContext,
        NewOrderMail, OrderContext, OrderMail, ShipmentMail,
    },
    send::{Branding, Message, Sources, branding, queue, render_for_sending, skip},
};

/// Every repository mailing reads or writes, so a host names one bound. The store's
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
/// It renders each message and records it `queued` (or `skipped`, its template off) for
/// the send job: no network, so a dispatch step's budget holds and no connection waits
/// on a provider. Each message is recorded once per (event, template, recipient): a run
/// that finds one queues nothing more, so running twice is harmless. It fails only for
/// storage or data errors (a missing order, a bad payload), which the dispatcher retries.
///
/// It holds the host's `MailEnv` (links, the store, the send job's wake-up), so a host
/// builds its handler set at startup rather than as a `const`.
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
        queue_event(&self.env, ports, event)
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

/// What an event is about, loaded once for all its messages.
struct About {
    branding: Branding,
    order: Order,
    payment: Option<Payment>,
    fulfillment: Option<Fulfillment>,
}

/// Queues what `event` calls for and returns the records, in order (earlier runs'
/// records included). An event the mailer doesn't answer, or one staff chose not to tell
/// the buyer about, queues nothing. Wakes the send job (`MailEnv::queued`) when it
/// queued something.
pub async fn queue_event<E: MailEnv, P: MailPorts>(
    env: &E,
    ports: &P,
    event: &StoredEvent,
) -> Result<Vec<Notification>, CommandError> {
    let template = match event.event_type.as_str() {
        ORDER_PLACED => TemplateKey::OrderConfirmation,
        ORDER_CANCELLED => TemplateKey::OrderCancelled,
        FULFILLMENT_CREATED => TemplateKey::OrderShipped,
        FULFILLMENT_UPDATED => TemplateKey::OrderTrackingUpdated,
        _ => return Ok(Vec::new()),
    };
    let event_id = EventId::from_uuid(event.id);
    let payload: Payload =
        serde_json::from_value(event.data.clone()).map_err(|e| CommandError::BadEvent {
            event: event_id,
            event_type: event.event_type.clone(),
            reason: e.to_string(),
        })?;
    if template != TemplateKey::OrderConfirmation && !payload.notify_customer {
        return Ok(Vec::new());
    }
    let order = load_order(ports, payload.order).await?;
    let fulfillment = match (template, payload.fulfillment) {
        (TemplateKey::OrderShipped | TemplateKey::OrderTrackingUpdated, Some(id)) => {
            Some(load_fulfillment(ports, id).await?)
        }
        _ => None,
    };
    let about = About {
        branding: branding(env, ports).await?,
        payment: newest_payment(ports, order.id()).await?,
        order,
        fulfillment,
    };
    let mut plan = vec![(template, vec![about.order.email().clone()])];
    if template == TemplateKey::OrderConfirmation {
        let staff = about.branding.settings.staff_recipients().to_vec();
        if !staff.is_empty() {
            plan.push((TemplateKey::StaffNewOrder, staff));
        }
    }
    let mut records = Vec::new();
    let mut queued = false;
    for (template, recipients) in plan {
        let (done, added) =
            queue_template(env, ports, &about, event_id, template, recipients).await?;
        records.extend(done);
        queued |= added;
    }
    if queued {
        env.queued();
    }
    Ok(records)
}

/// The records for `template` to each of `recipients` about `event`: an earlier run's
/// where there is one, else new ones, `queued` (rendered once for all of them) or
/// `skipped` when the template is off (not rendered). Says whether it queued any.
async fn queue_template<E: MailEnv, P: MailPorts>(
    env: &E,
    ports: &P,
    about: &About,
    event: EventId,
    template: TemplateKey,
    recipients: Vec<Email>,
) -> Result<(Vec<Notification>, bool), CommandError> {
    let mut slots = Vec::with_capacity(recipients.len());
    for to in recipients {
        let earlier = ports.notifications().find(event, template, &to).await?;
        slots.push((to, earlier));
    }
    if slots.iter().all(|(_, earlier)| earlier.is_some()) {
        return Ok((slots.into_iter().filter_map(|(_, e)| e).collect(), false));
    }
    let sources = Sources::load(ports, template).await?;
    let rendering = match sources.template.enabled {
        true => {
            let context = order_context(
                env,
                ports,
                &about.branding,
                template,
                &about.order,
                about.payment.as_ref(),
                about.fulfillment.as_ref(),
            )
            .await?;
            Some(render_for_sending(&sources, &context)?)
        }
        false => None,
    };
    let mut records = Vec::with_capacity(slots.len());
    let mut queued = false;
    for (to, earlier) in slots {
        if let Some(earlier) = earlier {
            records.push(earlier);
            continue;
        }
        let message = Message {
            template,
            to,
            event: Some(event),
            order: Some(about.order.id()),
        };
        // An erased customer's placeholder can't receive mail (design note 0032, CD10).
        if message.to.is_placeholder() {
            records.push(crate::send::skip_as(env, ports, &message, crate::send::ERASED).await?);
            continue;
        }
        records.push(match &rendering {
            Some(rendering) => {
                queued = true;
                queue(env, ports, &about.branding, &message, rendering).await?
            }
            None => skip(env, ports, &message).await?,
        });
    }
    Ok((records, queued))
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
/// is now, with its newest `payment`; the shipping templates need `fulfillment` (the
/// order's newest without one). Staff emails get no `order.url`: the link carries the
/// buyer's access token, which isn't staff's to forward (they have the admin's
/// `admin_url`).
pub async fn order_context<E: MailEnv, P: HasFulfillments>(
    env: &E,
    ports: &P,
    branding: &Branding,
    template: TemplateKey,
    order: &Order,
    payment: Option<&Payment>,
    fulfillment: Option<&Fulfillment>,
) -> Result<MailContext, CommandError> {
    let zone: Option<Timezone> = branding.store.timezone();
    let url = match template {
        TemplateKey::StaffNewOrder => None,
        _ => branding
            .settings
            .order_url()
            .map(|pattern| pattern.link(order.id(), &env.order_token(order.id()))),
    };
    let mail = OrderMail {
        shop: branding.shop.clone(),
        brand: branding.brand.clone(),
        order: OrderContext::of(order, payment, url, zone),
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
        | TemplateKey::StaffPasswordReset
        | TemplateKey::CustomerLoginCode => MailContext::Order(mail),
    })
}
