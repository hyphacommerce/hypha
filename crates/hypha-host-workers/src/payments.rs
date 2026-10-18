//! The payment providers this host is built with (design note 0027, PA5): the
//! composition root for providers, next to `handlers.rs`. A spore with a provider
//! (`spore_stripe`) adds it to the tuple; checkout and the method commands find one by
//! key and know none of them.

use hypha_payments::ManualProvider;

/// Every provider, in no particular order (keys must be unique).
pub type PaymentProviders = (ManualProvider,);

pub const PAYMENT_PROVIDERS: PaymentProviders = (ManualProvider,);
