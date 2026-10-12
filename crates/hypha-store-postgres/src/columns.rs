//! Conversions between domain values and their column representations.

use hypha_core::{Currency, RepoError, Version};

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
