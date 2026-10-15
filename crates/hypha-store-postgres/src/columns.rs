//! Conversions between domain values and their column representations.

use hypha_auth::TokenHash;
use hypha_core::{
    Actor, AddressParts, AgentId, ApiKeyId, Currency, RepoError, Scope, TaxRate, UserId, Version,
};
use tokio_postgres::Row;
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

/// An address's columns, in the order every helper below uses. Tables that keep one
/// address on the row prefix them (`shipping_`, `address_`); `customer_addresses` doesn't.
const ADDRESS_FIELDS: [&str; 10] = [
    "first_name",
    "last_name",
    "company",
    "line1",
    "line2",
    "city",
    "province",
    "postal_code",
    "country",
    "phone",
];

/// `p_first_name, p_last_name, …` for a SELECT list (read back with `address_from_row`).
pub(crate) fn address_columns(prefix: &str) -> String {
    ADDRESS_FIELDS.map(|f| format!("{prefix}{f}")).join(", ")
}

/// `p_first_name = $first, p_last_name = $first+1, …` for an UPDATE, bound to the
/// values of `address_values`.
pub(crate) fn address_assignments(prefix: &str, first: usize) -> String {
    (first..)
        .zip(ADDRESS_FIELDS)
        .map(|(n, f)| format!("{prefix}{f} = ${n}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The ten column values of an address, all NULL for none.
pub(crate) fn address_values(a: Option<&AddressParts>) -> [Option<&str>; 10] {
    [
        a.and_then(|p| p.first_name.as_deref()),
        a.map(|p| p.last_name.as_str()),
        a.and_then(|p| p.company.as_deref()),
        a.map(|p| p.line1.as_str()),
        a.and_then(|p| p.line2.as_deref()),
        a.map(|p| p.city.as_str()),
        a.and_then(|p| p.province.as_deref()),
        a.and_then(|p| p.postal_code.as_deref()),
        a.map(|p| p.country.as_str()),
        a.and_then(|p| p.phone.as_deref()),
    ]
}

/// The address in the ten columns from `first` of `row` (as `address_columns` lists
/// them), or `None` when they're NULL. Check constraints keep the required columns all
/// set or all NULL, so the last name stands for all of them.
pub(crate) fn address_from_row(row: &Row, first: usize) -> Option<AddressParts> {
    row.get::<_, Option<String>>(first + 1)
        .map(|last_name| AddressParts {
            first_name: row.get(first),
            last_name,
            company: row.get(first + 2),
            line1: row.get(first + 3),
            line2: row.get(first + 4),
            city: row.get(first + 5),
            province: row.get(first + 6),
            postal_code: row.get(first + 7),
            country: row.get(first + 8),
            phone: row.get(first + 9),
        })
}
