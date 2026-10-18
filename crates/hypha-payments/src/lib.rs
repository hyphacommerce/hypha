//! Payments (design note 0027): the methods a store offers, the payments an order has,
//! and the `PaymentProvider` port with its built-in `manual` provider (cash on
//! delivery, bank transfer).
//!
//! A leaf domain: it depends on core only. A payment names its order and method by ID;
//! choosing a method at checkout and keeping the order's `financial_status` in step
//! (`financial_status`) are the workflows' (`hypha-checkout`, `hypha-order-actions`),
//! which write a payment and its order in one batch.

pub mod commands;
pub mod error;
pub mod events;
pub mod method;
pub mod payment;
pub mod provider;
pub mod repo;

pub use commands::{
    CommandError, CreatePaymentMethod, UpdatePaymentMethod, create_payment_method,
    delete_payment_method, get_payment_method, list_payment_methods, update_payment_method,
};
pub use error::PaymentError;
pub use events::{PaymentCreated, PaymentPaid, PaymentVoided};
pub use method::{NewPaymentMethod, PaymentMethod, PaymentMethodSnapshot};
pub use payment::{NewPayment, Payment, PaymentSnapshot, PaymentStatus, financial_status};
pub use provider::{
    ManualProvider, NextAction, PaymentProvider, PaymentProviders, PaymentRequest, ProviderKey,
};
pub use repo::{
    HasPaymentMethods, HasPayments, PaymentMethodRepo, PaymentRepo, StagePayments, keys,
    newest_payment,
};
