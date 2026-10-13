use hypha_core::{ErrorCode, TaxClassId};
use thiserror::Error;

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum TaxError {
    #[error("tax class {0} has more than one rate in this region")]
    DuplicateClassRate(TaxClassId),
    #[error("tax class {0} has no rate in this region")]
    ClassRateNotFound(TaxClassId),
}

impl ErrorCode for TaxError {
    fn code(&self) -> &'static str {
        match self {
            TaxError::DuplicateClassRate(..) => "duplicate_class_rate",
            TaxError::ClassRateNotFound(..) => "class_rate_not_found",
        }
    }
}
