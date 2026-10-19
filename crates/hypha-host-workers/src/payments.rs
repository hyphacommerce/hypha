//! The payment providers core builds in (design note 0027, PA5): the composition root
//! for providers, next to `handlers.rs`. Apps collect payments under their own keys
//! (`app_stripe`, design note 0029, §3.1), answered by `hypha-http`'s `HostPayments`
//! beside these; a spore adds its own (`Spore::PaymentProviders`), joined to these by
//! `WorkersStorage` (design note 0030, §4). Checkout and the method commands find one by
//! key and know none of them.

use hypha_payments::ManualProvider;

/// Core's providers, in no particular order (keys must be unique, spores' included).
pub type CoreProviders = (ManualProvider,);

pub const CORE_PROVIDERS: CoreProviders = (ManualProvider,);
