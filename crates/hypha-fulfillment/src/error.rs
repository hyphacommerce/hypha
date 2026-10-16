use hypha_core::{ErrorCode, FulfillmentId, OrderLineId, TextError};
use thiserror::Error;

use crate::{Fulfillment, Tracking};

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum FulfillmentError {
    #[error(transparent)]
    Text(#[from] TextError),
    #[error("a fulfilment needs at least one line")]
    NoLines,
    #[error("order line {0} is listed more than once")]
    DuplicateLine(OrderLineId),
    #[error("{count} tracking numbers; at most {max}", max = Fulfillment::MAX_TRACKING)]
    TooMuchTracking { count: usize },
    #[error("tracking URL {0:?} isn't an http:// or https:// URL of at most {max} characters", max = Tracking::MAX_URL_LEN)]
    InvalidTrackingUrl(String),
    #[error("fulfilment {0} is cancelled")]
    Cancelled(FulfillmentId),
}

impl ErrorCode for FulfillmentError {
    fn code(&self) -> &'static str {
        match self {
            FulfillmentError::Text(e) => e.code(),
            FulfillmentError::NoLines => "fulfillment_has_no_lines",
            FulfillmentError::DuplicateLine(_) => "duplicate_fulfillment_line",
            FulfillmentError::TooMuchTracking { .. } => "too_much_tracking",
            FulfillmentError::InvalidTrackingUrl(_) => "invalid_tracking_url",
            FulfillmentError::Cancelled(_) => "fulfillment_cancelled",
        }
    }
}
