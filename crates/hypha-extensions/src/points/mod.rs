//! The catalog of extension points this build knows (design note 0029, §3.1). A new
//! point is a type implementing `ExtensionPoint` here, with its request and response,
//! and a line in the extensions document's list (`hypha-http`'s `openapi.rs`).

pub mod payments;
pub mod search;
pub mod shipping;

pub use payments::{
    NextAction, PaymentActionRequest, PaymentActionResponse, PaymentStartRequest,
    PaymentStartResponse, PaymentsCapture, PaymentsRefund, PaymentsStart, PaymentsVoid,
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

/// Every point this build knows, as its name and version: what an app may register
/// (design note 0029, §3, "Registering").
pub const KNOWN: &[(&str, u16)] = &[
    (ShippingRates::NAME, ShippingRates::VERSION),
    (PaymentsStart::NAME, PaymentsStart::VERSION),
    (PaymentsCapture::NAME, PaymentsCapture::VERSION),
    (PaymentsVoid::NAME, PaymentsVoid::VERSION),
    (PaymentsRefund::NAME, PaymentsRefund::VERSION),
    (SearchQuery::NAME, SearchQuery::VERSION),
];
