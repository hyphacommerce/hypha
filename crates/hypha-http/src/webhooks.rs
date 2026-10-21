//! Webhooks (design note 0016, section 8): the event envelope apps receive, and the
//! sender `hypha-dispatch` calls through its `WebhookSender` port. Calls are signed and
//! guarded like rate callbacks (`outbound.rs`).

use std::time::Duration;

use hypha_apps::{App, open_secret};
use hypha_core::{
    Actor, AppId, AssetId, CatalogId, CategoryId, ChannelId, Clock, CustomerId, Delivery,
    DeliveryId, EventId, FulfillmentId, InventoryItemId, LocationId, MarketId, OrderId, PaymentId,
    PriceListId, ProductId, ShippingZoneId, StoreId, StoredEvent, TaxClassId, TaxRegionId,
};
use hypha_dispatch::WebhookSender;
use serde::Serialize;
use uuid::Uuid;

use crate::{
    error::ApiError,
    host::{HostCalls, HostClock},
    outbound::{CallError, Method, OutboundPolicy, SignedCall},
    routes::rfc3339,
    state::HostEnv,
};

/// The event's ID (`evt_…`), the same in every delivery and retry of it.
pub const EVENT_ID_HEADER: &str = "Hypha-Event-Id";
/// Its type (`order.placed`), to route on without parsing the body.
pub const EVENT_TYPE_HEADER: &str = "Hypha-Event-Type";
/// The delivery's ID (`dlv_…`): stable across retries, for de-duplicating.
pub const DELIVERY_ID_HEADER: &str = "Hypha-Delivery-Id";
/// The store the event is from (`store_…`), as extension calls name it: an app
/// installed in several stores picks the secret to verify with before reading the body.
pub use hypha_extensions::STORE_HEADER;

/// An event as apps receive it (design note 0016, EV5), and as
/// `GET /admin/events/{id}` shows it.
#[derive(Serialize, utoipa::ToSchema)]
pub struct EventEnvelope {
    pub id: EventId,
    /// Dotted: `product.variant_removed`.
    #[serde(rename = "type")]
    pub event_type: String,
    /// The payload's version: a reshaped payload is a new version of the same type.
    pub version: u16,
    pub subject: EventSubject,
    pub actor: EventActor,
    /// When the change committed (RFC 3339).
    pub occurred_at: String,
    /// The facts of the change; see the event catalog.
    #[schema(value_type = Object)]
    pub data: serde_json::Value,
}

/// What an event is about.
#[derive(Serialize, utoipa::ToSchema)]
pub struct EventSubject {
    /// `product`, `order`, …
    #[serde(rename = "type")]
    pub kind: String,
    /// Its prefixed ID (`prod_…`).
    pub id: String,
    /// Its version after the change, when it has one: deliveries can arrive out of
    /// order, and a consumer drops an event older than one it applied (EV7).
    pub version: Option<u64>,
}

/// Who caused an event.
#[derive(Serialize, utoipa::ToSchema)]
pub struct EventActor {
    /// `user`, `agent`, `api_key`, `app` or `system`.
    #[serde(rename = "type")]
    pub kind: &'static str,
    /// Their prefixed ID; `null` for the system.
    pub id: Option<String>,
}

impl EventEnvelope {
    pub(crate) fn new(event: &StoredEvent) -> Result<Self, ApiError> {
        Ok(Self {
            id: EventId::from_uuid(event.id),
            event_type: event.event_type.clone(),
            version: event.version,
            subject: EventSubject {
                kind: event.subject_kind.clone(),
                id: subject_id(&event.subject_kind, event.subject_id),
                version: event.subject_version.map(|v| v.value()),
            },
            actor: actor_of(event.actor),
            occurred_at: rfc3339(event.occurred_at)?,
            data: event.data.clone(),
        })
    }
}

/// A subject's ID as the API spells it: prefixed for every kind with an ID type, the
/// bare UUID for any other (`search_settings`, a store's one set, is the nil UUID).
fn subject_id(kind: &str, id: Uuid) -> String {
    let prefix = match kind {
        "product" => ProductId::PREFIX,
        "catalog" => CatalogId::PREFIX,
        "price_list" => PriceListId::PREFIX,
        "market" => MarketId::PREFIX,
        "tax_class" => TaxClassId::PREFIX,
        "tax_region" => TaxRegionId::PREFIX,
        "inventory_item" => InventoryItemId::PREFIX,
        "customer" => CustomerId::PREFIX,
        "customer_group" => hypha_core::CustomerGroupId::PREFIX,
        "order" => OrderId::PREFIX,
        "shipping_zone" => ShippingZoneId::PREFIX,
        "app" => AppId::PREFIX,
        "channel" => ChannelId::PREFIX,
        "category" => CategoryId::PREFIX,
        "location" => LocationId::PREFIX,
        "fulfillment" => FulfillmentId::PREFIX,
        "payment" => PaymentId::PREFIX,
        "asset" => AssetId::PREFIX,
        _ => return id.to_string(),
    };
    format!("{prefix}_{}", id.simple())
}

/// An actor as events and grants show it: its kind and prefixed ID.
pub fn actor_of(actor: Actor) -> EventActor {
    let (kind, id) = match actor {
        Actor::User(id) => ("user", Some(id.to_string())),
        Actor::Agent(id) => ("agent", Some(id.to_string())),
        Actor::ApiKey(id) => ("api_key", Some(id.to_string())),
        Actor::App(id) => ("app", Some(id.to_string())),
        Actor::Customer(id) => ("customer", Some(id.to_string())),
        Actor::System => ("system", None),
    };
    EventActor { kind, id }
}

/// Sends webhooks for the dispatcher: the host's calls, its key to open apps' secrets,
/// and its clock for the signature's time.
pub struct HostWebhooks<'a> {
    pub calls: &'a HostCalls,
    pub outbound: OutboundPolicy,
    pub env: &'a HostEnv,
    pub clock: &'a HostClock,
    /// Named in every delivery's `Hypha-Store`.
    pub store: StoreId,
}

impl WebhookSender for HostWebhooks<'_> {
    async fn send(&self, app: &App, delivery: &Delivery) -> Result<(), String> {
        let webhook = app
            .webhook()
            .ok_or_else(|| format!("app {} has no webhook", app.key()))?;
        let mut envelope = EventEnvelope::new(&delivery.event).map_err(|e| {
            tracing::error!(event = %delivery.event.id, error = ?e, "webhooks: bad event");
            "the event can't be sent".to_string()
        })?;
        // Personal data only as far as the app's grant allows (design note 0029, AP10).
        envelope.data = crate::redact::event_data(
            |scope| app.grant().is_some_and(|g| g.has(scope)),
            &delivery.event.data,
        );
        let body = serde_json::to_vec(&envelope).map_err(|e| e.to_string())?;
        // Another `HYPHA_SECRET_KEY` than the one that sealed it: the operator's to fix.
        let secret = open_secret(self.env, app).map_err(|e| {
            tracing::error!(app = %app.key(), error = %e, "webhooks: can't open the secret");
            "the app's secret can't be opened".to_string()
        })?;
        let headers = [
            (EVENT_ID_HEADER, envelope.id.to_string()),
            (EVENT_TYPE_HEADER, envelope.event_type.clone()),
            (
                DELIVERY_ID_HEADER,
                DeliveryId::from_uuid(delivery.id).to_string(),
            ),
            (STORE_HEADER, self.store.to_string()),
        ];
        let call = SignedCall {
            method: Method::Post,
            url: webhook.url.as_str(),
            timeout: Duration::from_millis(u64::from(webhook.timeout_ms)),
            secret: &secret,
            at: self.clock.now().unix_micros() / 1_000_000,
            headers: &headers,
            body,
            discard_answer: true,
        };
        let sent = self.calls.send_signed(self.outbound, call).await;
        sent.map(|_| ()).map_err(|e| match e {
            CallError::Timeout => format!("no answer within {} ms", webhook.timeout_ms),
            other => other.reason(),
        })
    }
}

#[cfg(test)]
mod tests {
    use hypha_core::{Timestamp, UserId, Version};

    use super::*;

    #[test]
    fn the_envelope_names_ids_with_their_prefixes() {
        let event = StoredEvent {
            seq: 1,
            id: Uuid::from_u128(1),
            event_type: "product.created".to_string(),
            version: 1,
            subject_kind: "product".to_string(),
            subject_id: Uuid::from_u128(2),
            subject_version: Some(Version::NEW.next()),
            actor: Actor::User(UserId::from_uuid(Uuid::from_u128(3))),
            occurred_at: Timestamp::from_unix_micros(1_800_000_000_000_000),
            data: serde_json::json!({"handle": "mug"}),
        };
        let json = serde_json::to_value(EventEnvelope::new(&event).unwrap()).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "id": "evt_00000000000000000000000000000001",
                "type": "product.created",
                "version": 1,
                "subject": {
                    "type": "product",
                    "id": "prod_00000000000000000000000000000002",
                    "version": 1
                },
                "actor": {"type": "user", "id": "user_00000000000000000000000000000003"},
                "occurred_at": "2027-01-15T08:00:00Z",
                "data": {"handle": "mug"}
            })
        );
    }

    #[test]
    fn every_kind_with_an_id_type_is_prefixed() {
        let id = Uuid::from_u128(2);
        let kinds = [
            (hypha_products::events::PRODUCT, ProductId::PREFIX),
            (hypha_markets::events::CATALOG, CatalogId::PREFIX),
            (hypha_markets::events::PRICE_LIST, PriceListId::PREFIX),
            (hypha_markets::events::MARKET, MarketId::PREFIX),
            (hypha_tax::events::TAX_CLASS, TaxClassId::PREFIX),
            (hypha_tax::events::TAX_REGION, TaxRegionId::PREFIX),
            (
                hypha_inventory::events::INVENTORY_ITEM,
                InventoryItemId::PREFIX,
            ),
            (hypha_inventory::events::LOCATION, LocationId::PREFIX),
            (hypha_customers::events::CUSTOMER, CustomerId::PREFIX),
            (hypha_orders::events::ORDER, OrderId::PREFIX),
            (
                hypha_fulfillment::events::FULFILLMENT,
                FulfillmentId::PREFIX,
            ),
            (hypha_payments::events::PAYMENT, PaymentId::PREFIX),
            (
                hypha_shipping::events::SHIPPING_ZONE,
                ShippingZoneId::PREFIX,
            ),
            (hypha_apps::APP, AppId::PREFIX),
            (hypha_channels::events::CHANNEL, ChannelId::PREFIX),
            (hypha_categories::events::CATEGORY, CategoryId::PREFIX),
        ];
        for (kind, prefix) in kinds {
            assert_eq!(
                subject_id(kind, id),
                format!("{prefix}_00000000000000000000000000000002"),
                "{kind}"
            );
        }
        assert_eq!(
            subject_id(hypha_search_index::SEARCH_SETTINGS, Uuid::nil()),
            "00000000-0000-0000-0000-000000000000"
        );
    }

    #[test]
    fn a_kind_without_a_prefix_keeps_its_uuid() {
        assert_eq!(
            subject_id("spore_thing", Uuid::from_u128(2)),
            "00000000-0000-0000-0000-000000000002"
        );
        let system = actor_of(Actor::System);
        assert_eq!((system.kind, system.id), ("system", None));
    }
}
