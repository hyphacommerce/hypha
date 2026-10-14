//! Conversions between domain values and their column representations.

use hypha_auth::TokenHash;
use hypha_core::{Actor, AgentId, ApiKeyId, Currency, RepoError, Scope, TaxRate, UserId, Version};
use uuid::Uuid;

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

/// An actor as `(kind, id)` columns; the system actor has no ID and is stored as nil.
pub(crate) fn actor_to_db(actor: Actor) -> (&'static str, Uuid) {
    match actor {
        Actor::System => ("system", Uuid::nil()),
        Actor::User(id) => ("user", id.as_uuid()),
        Actor::Agent(id) => ("agent", id.as_uuid()),
        Actor::ApiKey(id) => ("api_key", id.as_uuid()),
    }
}

pub(crate) fn actor_from_db(kind: &str, id: Uuid) -> Result<Actor, RepoError> {
    match kind {
        "system" => Ok(Actor::System),
        "user" => Ok(Actor::User(UserId::from_uuid(id))),
        "agent" => Ok(Actor::Agent(AgentId::from_uuid(id))),
        "api_key" => Ok(Actor::ApiKey(ApiKeyId::from_uuid(id))),
        other => Err(corrupt(format!("unknown actor kind {other:?}"))),
    }
}

/// Scopes as their JSON names (`products:write`), so the column reads like the API.
pub(crate) fn scopes_to_db(scopes: &[Scope]) -> Vec<&'static str> {
    scopes.iter().map(|s| s.as_str()).collect()
}

pub(crate) fn scopes_from_db(names: Vec<String>) -> Result<Vec<Scope>, RepoError> {
    names
        .iter()
        .map(|name| {
            Scope::ALL
                .into_iter()
                .find(|s| s.as_str() == name)
                .ok_or_else(|| corrupt(format!("unknown scope {name:?}")))
        })
        .collect()
}

pub(crate) fn token_hash_from_db(bytes: Vec<u8>) -> Result<TokenHash, RepoError> {
    <[u8; 32]>::try_from(bytes)
        .map(TokenHash::from_bytes)
        .map_err(|_| corrupt("token hash is not 32 bytes"))
}

/// SQL turning parameter `$n` (microseconds since the epoch, `bigint`, maybe null)
/// into a `timestamptz`.
pub(crate) fn ts(n: usize) -> String {
    format!("('epoch'::timestamptz + ${n}::bigint * interval '1 microsecond')")
}

/// SQL reading a `timestamptz` column back as microseconds since the epoch.
pub(crate) fn micros(column: &str) -> String {
    format!("(extract(epoch FROM {column}) * 1000000)::bigint")
}
