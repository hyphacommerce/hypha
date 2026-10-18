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
