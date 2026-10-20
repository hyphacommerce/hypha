//! What happens to payments, as events (design note 0027, §6). Contract types: a
//! reshaped field is a new `VERSION`, never an edit.

use hypha_core::{CartId, Event, Money, OrderId, PaymentId, PaymentMethodId};
use serde::Serialize;

pub const PAYMENT: &str = "payment";

/// `payment.created`: checkout started collecting `amount` for a cart (an online
/// method, before the order: design note 0031) or an order (a manual method), or staff
/// recorded money on an order without a payment.
#[derive(Debug, Serialize)]
pub struct PaymentCreated {
    pub payment: PaymentId,
    /// The cart it was started for (design note 0031); `null` for money staff recorded
    /// on an order by hand.
    pub cart: Option<CartId>,
    /// `null` until checkout placed the order (an online payment is started for the
    /// cart, before the order exists).
    pub order: Option<OrderId>,
    /// `null` for a payment staff recorded on an order placed without a method.
    pub method: Option<PaymentMethodId>,
    pub provider: String,
    pub amount: Money,
}

impl Event for PaymentCreated {
    const TYPE: &'static str = "payment.created";
    const VERSION: u16 = 2;
}

/// `payment.paid`: the money arrived (staff recorded it, for manual payments; its app
/// reported it, for an app's).
#[derive(Debug, Serialize)]
pub struct PaymentPaid {
    pub payment: PaymentId,
    /// The cart it was started for (design note 0031); `null` for money staff recorded
    /// on an order by hand.
    pub cart: Option<CartId>,
    /// `null` until checkout placed the order (an online payment is started for the
    /// cart, before the order exists).
    pub order: Option<OrderId>,
    pub method: Option<PaymentMethodId>,
    pub provider: String,
    pub amount: Money,
    /// What staff typed (a bank reference) or the provider's ID.
    pub reference: Option<String>,
}

impl Event for PaymentPaid {
    const TYPE: &'static str = "payment.paid";
    const VERSION: u16 = 2;
}

/// `payment.voided`: cancelled before any money moved (its order was cancelled).
#[derive(Debug, Serialize)]
pub struct PaymentVoided {
    pub payment: PaymentId,
    /// The cart it was started for (design note 0031); `null` for money staff recorded
    /// on an order by hand.
    pub cart: Option<CartId>,
    /// `null` until checkout placed the order (an online payment is started for the
    /// cart, before the order exists).
    pub order: Option<OrderId>,
    pub method: Option<PaymentMethodId>,
    pub provider: String,
    pub amount: Money,
}

impl Event for PaymentVoided {
    const TYPE: &'static str = "payment.voided";
    const VERSION: u16 = 2;
}

/// `payment.authorized`: the provider holds the money (a card authorization), not yet
/// captured.
#[derive(Debug, Serialize)]
pub struct PaymentAuthorized {
    pub payment: PaymentId,
    /// The cart it was started for (design note 0031); `null` for money staff recorded
    /// on an order by hand.
    pub cart: Option<CartId>,
    /// `null` until checkout placed the order (an online payment is started for the
    /// cart, before the order exists).
    pub order: Option<OrderId>,
    pub method: Option<PaymentMethodId>,
    pub provider: String,
    pub amount: Money,
    /// The provider's ID for the payment.
    pub reference: Option<String>,
}

impl Event for PaymentAuthorized {
    const TYPE: &'static str = "payment.authorized";
    const VERSION: u16 = 2;
}

/// `payment.processing`: the provider took the shopper's payment and is collecting it
/// (SEPA Direct Debit, some bank methods: days); committed, not collected yet.
#[derive(Debug, Serialize)]
pub struct PaymentProcessing {
    pub payment: PaymentId,
    pub cart: Option<CartId>,
    pub order: Option<OrderId>,
    pub method: Option<PaymentMethodId>,
    pub provider: String,
    pub amount: Money,
    pub reference: Option<String>,
}

impl Event for PaymentProcessing {
    const TYPE: &'static str = "payment.processing";
    const VERSION: u16 = 2;
}

/// `payment.failed`: the provider declined it; the buyer may try again, which makes a
/// new payment.
#[derive(Debug, Serialize)]
pub struct PaymentFailed {
    pub payment: PaymentId,
    /// The cart it was started for (design note 0031); `null` for money staff recorded
    /// on an order by hand.
    pub cart: Option<CartId>,
    /// `null` until checkout placed the order (an online payment is started for the
    /// cart, before the order exists).
    pub order: Option<OrderId>,
    pub method: Option<PaymentMethodId>,
    pub provider: String,
    pub amount: Money,
    pub reference: Option<String>,
}

impl Event for PaymentFailed {
    const TYPE: &'static str = "payment.failed";
    const VERSION: u16 = 2;
}

/// `payment.refunded`: `amount` was given back, `refunded` in all so far; `status` is
/// `partially_refunded`, or `refunded` once all of it was.
#[derive(Debug, Serialize)]
pub struct PaymentRefunded {
    pub payment: PaymentId,
    /// The cart it was started for (design note 0031); `null` for money staff recorded
    /// on an order by hand.
    pub cart: Option<CartId>,
    /// `null` until checkout placed the order (an online payment is started for the
    /// cart, before the order exists).
    pub order: Option<OrderId>,
    pub method: Option<PaymentMethodId>,
    pub provider: String,
    /// This refund.
    pub amount: Money,
    /// Every refund so far, this one included.
    pub refunded: Money,
    pub status: &'static str,
    /// The provider's ID for the payment, when it gave one.
    pub reference: Option<String>,
    /// The provider's ID for this refund, when it gave one (an app always does).
    pub refund: Option<String>,
}

impl Event for PaymentRefunded {
    const TYPE: &'static str = "payment.refunded";
    const VERSION: u16 = 2;
}
