//! What the buyer is told (design note 0018, section 3). Hosts render and send them
//! through `hypha_core::Notifier<BuyerEmail>` after the action commits, outside the
//! request; for now they're logged, like staff emails.

use hypha_core::Email;
use hypha_fulfillment::{Fulfillment, Tracking};
use hypha_orders::{CancelReason, Order, OrderNumber};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuyerEmail {
    /// Part of the order left, with its tracking.
    Shipped {
        to: Email,
        order: OrderNumber,
        tracking: Vec<Tracking>,
    },
    /// A shipment's tracking changed.
    TrackingUpdated {
        to: Email,
        order: OrderNumber,
        tracking: Vec<Tracking>,
    },
    Cancelled {
        to: Email,
        order: OrderNumber,
        reason: CancelReason,
    },
}

impl BuyerEmail {
    pub fn shipped(order: &Order, fulfillment: &Fulfillment) -> Self {
        BuyerEmail::Shipped {
            to: order.email().clone(),
            order: order.number(),
            tracking: fulfillment.tracking().to_vec(),
        }
    }

    pub fn tracking_updated(order: &Order, fulfillment: &Fulfillment) -> Self {
        BuyerEmail::TrackingUpdated {
            to: order.email().clone(),
            order: order.number(),
            tracking: fulfillment.tracking().to_vec(),
        }
    }

    /// `None` for an order that isn't cancelled.
    pub fn cancelled(order: &Order) -> Option<Self> {
        order.cancelled().map(|c| BuyerEmail::Cancelled {
            to: order.email().clone(),
            order: order.number(),
            reason: c.reason,
        })
    }
}
