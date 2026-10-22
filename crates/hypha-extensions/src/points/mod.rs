//! The catalog of extension points this build knows. A new point is a type implementing
//! `ExtensionPoint` here, with its request and response, and a line in the extensions
//! document's list (`hypha-http`'s `openapi.rs`).

pub mod admin;
pub mod payments;
pub mod search;
pub mod shipping;

pub use admin::{
    ActionContext, AdminActionRequest, AdminActionResponse, MAX_ACTION_MESSAGE, RunAdminAction,
};

pub use payments::{
    NextAction, PaymentActionRequest, PaymentActionResponse, PaymentStartRequest,
    PaymentStartResponse, PaymentState, PaymentStatusRequest, PaymentStatusResponse,
    PaymentsCapture, PaymentsRefund, PaymentsStart, PaymentsStatus, PaymentsVoid,
};
pub use search::{
    SearchFacetCounts, SearchFacetValue, SearchFilter, SearchGroup, SearchHit, SearchPriceRange,
    SearchQuery, SearchQueryRequest, SearchQueryResponse, SearchSort,
};
pub use shipping::{
    PostalAddress, QuotedRate, RateCallbackRequest, RateEstimate, RateItemBody, RateRequestBody,
    RatesResponse, ShippingRates,
};

use crate::ExtensionPoint;

use hypha_core::Scope;

/// A point this build knows: its name, version and the scope a grant needs for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Known {
    pub name: &'static str,
    pub version: u16,
    pub scope: Scope,
}

const fn known<P: ExtensionPoint>() -> Known {
    Known {
        name: P::NAME,
        version: P::VERSION,
        scope: P::SCOPE,
    }
}

/// Every point this build knows: what an app may register ("Registering").
pub const KNOWN: &[Known] = &[
    known::<ShippingRates>(),
    known::<PaymentsStart>(),
    known::<PaymentsStatus>(),
    known::<PaymentsCapture>(),
    known::<PaymentsVoid>(),
    known::<PaymentsRefund>(),
    known::<SearchQuery>(),
];

/// The point named `name`, when this build knows it.
pub fn known_point(name: &str) -> Option<Known> {
    KNOWN.iter().copied().find(|k| k.name == name)
}
