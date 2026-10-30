use hypha_core::{ErrorCode, TaxClassId};
use thiserror::Error;

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum TaxError {
    #[error("tax class {0} has no rate in this region")]
    ClassRateNotFound(TaxClassId),
}

/// Why stored data can't be rebuilt into a tax region. Only `TaxRegion::restore` returns
/// it and adapters report it as a backend failure, so it has no API code.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum RestoreError {
    #[error("tax class {0} has more than one rate in this region")]
    DuplicateClassRate(TaxClassId),
}

impl ErrorCode for TaxError {
    fn code(&self) -> &'static str {
        match self {
            TaxError::ClassRateNotFound(..) => "class_rate_not_found",
        }
    }
}
