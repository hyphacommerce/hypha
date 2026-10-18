//! What happens to payments, as events (design note 0027, §6). Contract types: a
//! reshaped field is a new `VERSION`, never an edit.

use hypha_core::{Event, Money, OrderId, PaymentId, PaymentMethodId};
use serde::Serialize;

pub const PAYMENT: &str = "payment";

/// `payment.created`: checkout (or staff recording money on an order without one)
/// started collecting `amount` for `order`.
#[derive(Debug, Serialize)]
pub struct PaymentCreated {
    pub payment: PaymentId,
    pub order: OrderId,
    /// `null` for a payment staff recorded on an order placed without a method.
    pub method: Option<PaymentMethodId>,
    pub provider: String,
    pub amount: Money,
}

impl Event for PaymentCreated {
    const TYPE: &'static str = "payment.created";
    const VERSION: u16 = 1;
}

/// `payment.paid`: the money arrived (staff recorded it, for manual payments).
#[derive(Debug, Serialize)]
pub struct PaymentPaid {
    pub payment: PaymentId,
    pub order: OrderId,
    pub method: Option<PaymentMethodId>,
    pub provider: String,
    pub amount: Money,
    /// What staff typed (a bank reference) or the provider's ID.
    pub reference: Option<String>,
}

impl Event for PaymentPaid {
    const TYPE: &'static str = "payment.paid";
    const VERSION: u16 = 1;
}

/// `payment.voided`: cancelled before any money moved (its order was cancelled).
#[derive(Debug, Serialize)]
pub struct PaymentVoided {
    pub payment: PaymentId,
    pub order: OrderId,
    pub method: Option<PaymentMethodId>,
    pub provider: String,
    pub amount: Money,
}

impl Event for PaymentVoided {
    const TYPE: &'static str = "payment.voided";
    const VERSION: u16 = 1;
}
