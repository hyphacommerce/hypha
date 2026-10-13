//! Conversions between domain values and their column representations.

use hypha_core::{Currency, RepoError, TaxRate, Version};

use crate::error::corrupt;

/// Versions are `u64` in the domain and `bigint` in the database.
pub(crate) fn version_to_db(version: Version) -> Result<i64, RepoError> {
    i64::try_from(version.value()).map_err(|_| corrupt(format!("version {version:?} too large")))
}

pub(crate) fn version_from_db(version: i64) -> Result<Version, RepoError> {
    u64::try_from(version)
        .map(Version::new)
        .map_err(|_| corrupt(format!("negative version {version}")))
}

pub(crate) fn currency_from_db(code: &str) -> Result<Currency, RepoError> {
    code.parse().map_err(corrupt)
}

pub(crate) fn tax_rate_to_db(rate: TaxRate) -> Result<i32, RepoError> {
    i32::try_from(rate.value()).map_err(|_| corrupt(format!("tax rate {rate:?} too large")))
}

pub(crate) fn tax_rate_from_db(ppm: i32) -> Result<TaxRate, RepoError> {
    let ppm = u32::try_from(ppm).map_err(|_| corrupt(format!("negative tax rate {ppm}")))?;
    TaxRate::new(ppm).map_err(corrupt)
}
