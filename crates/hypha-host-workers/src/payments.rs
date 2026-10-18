//! The payment providers this host is built with (design note 0027, PA5): the
//! composition root for providers, next to `handlers.rs`. Apps collect payments under
//! their own keys (`app_stripe`, design note 0029, §3.1), answered by `hypha-http`'s
//! `HostPayments` beside these; a host that compiles in its own provider (0029, §12's
//! escape hatch) adds it to the tuple. Checkout and the method commands find one by key
//! and know none of them.

use hypha_payments::ManualProvider;

/// Every provider, in no particular order (keys must be unique).
pub type PaymentProviders = (ManualProvider,);

pub const PAYMENT_PROVIDERS: PaymentProviders = (ManualProvider,);
