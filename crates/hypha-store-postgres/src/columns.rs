//! Conversions between domain values and their column representations.

use hypha_auth::TokenHash;
use hypha_core::{
    Actor, AddressParts, AgentId, ApiKeyId, Currency, RepoError, Scope, Stamps, TaxRate, Timestamp,
    UserId, Version,
};
use tokio_postgres::{
    Row,
    types::{FromSql, ToSql, Type},
};
use uuid::Uuid;

use crate::error::corrupt;

/// The value of `row`'s column `name`, or `corrupt` naming the column when it's missing
/// or holds another type (a NULL in a non-`Option` type included). Rows are read with
/// this rather than `Row::get`, which panics on all of those, and by name rather than by
/// position, so a changed SELECT list can't shift what a column means.
pub(crate) fn col<'a, T: FromSql<'a>>(row: &'a Row, name: &str) -> Result<T, RepoError> {
    row.try_get(name)
        .map_err(|e| corrupt(format!("column {name:?}: {e}")))
}

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
        Actor::App(id) => ("app", id.as_uuid()),
    }
}

pub(crate) fn actor_from_db(kind: &str, id: Uuid) -> Result<Actor, RepoError> {
    match kind {
        "system" => Ok(Actor::System),
        "user" => Ok(Actor::User(UserId::from_uuid(id))),
        "agent" => Ok(Actor::Agent(AgentId::from_uuid(id))),
        "api_key" => Ok(Actor::ApiKey(ApiKeyId::from_uuid(id))),
        "app" => Ok(Actor::App(hypha_core::AppId::from_uuid(id))),
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

/// A nullable timestamp as the microseconds `ts` takes (`NULL` stays `NULL`).
pub(crate) fn opt_micros(t: Option<Timestamp>) -> Option<i64> {
    t.map(Timestamp::unix_micros)
}

/// A nullable `micros` column back as a timestamp.
pub(crate) fn opt_ts(micros: Option<i64>) -> Option<Timestamp> {
    micros.map(Timestamp::from_unix_micros)
}

/// SQL reading a `timestamptz` column back as microseconds since the epoch, for a SELECT
/// list: named after the column (without any `table.` prefix), so `col` finds it.
pub(crate) fn micros(column: &str) -> String {
    format!(
        "(extract(epoch FROM {column}) * 1000000)::bigint AS {}",
        unqualified(column)
    )
}

/// A SELECT list for one table of a join: `table`'s `plain` columns and its
/// `timestamps` (read as `micros`), each named `{prefix}{column}` so the tables' columns
/// don't clash; `Prefixed` reads them back.
pub(crate) fn prefixed_columns(
    table: &str,
    prefix: &str,
    plain: &[&str],
    timestamps: &[&str],
) -> String {
    let plain = plain.iter().map(|c| format!("{table}.{c} AS {prefix}{c}"));
    let timestamps = timestamps
        .iter()
        .map(|c| format!("(extract(epoch FROM {table}.{c}) * 1000000)::bigint AS {prefix}{c}"));
    plain.chain(timestamps).collect::<Vec<_>>().join(", ")
}

/// A row's columns under a prefix (`prefixed_columns`), or plain names with `""`.
#[derive(Clone, Copy)]
pub(crate) struct Prefixed<'a> {
    pub row: &'a Row,
    pub prefix: &'a str,
}

impl<'a> Prefixed<'a> {
    pub fn plain(row: &'a Row) -> Self {
        Prefixed { row, prefix: "" }
    }

    pub fn col<T: FromSql<'a>>(&self, name: &str) -> Result<T, RepoError> {
        if self.prefix.is_empty() {
            col(self.row, name)
        } else {
            col(self.row, &format!("{}{name}", self.prefix))
        }
    }
}

/// `placed_at` for `o.placed_at`.
fn unqualified(column: &str) -> &str {
    column.rsplit_once('.').map_or(column, |(_, name)| name)
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

/// How many columns (and parameters) an address takes: what comes after one is numbered
/// from its first plus this.
pub(crate) const ADDRESS_LEN: usize = ADDRESS_FIELDS.len();

/// A typed query parameter.
pub(crate) type Param<'a> = (&'a (dyn ToSql + Sync), Type);

/// `[$1 = id, $2.. = the address]`, the parameters of an `UPDATE … SET
/// {address_assignments(prefix, 2)} … WHERE id = $1`. Parameters pushed after these are
/// numbered from `2 + ADDRESS_LEN`.
pub(crate) fn id_and_address<'a>(
    id: &'a Uuid,
    address: &'a [Option<&'a str>; ADDRESS_LEN],
) -> Vec<Param<'a>> {
    let mut params: Vec<Param<'a>> = vec![(id, Type::UUID)];
    params.extend(
        address
            .iter()
            .map(|v| (v as &(dyn ToSql + Sync), Type::TEXT)),
    );
    params
}

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
pub(crate) fn address_values(a: Option<&AddressParts>) -> [Option<&str>; ADDRESS_LEN] {
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

/// The address in `row`'s columns with this prefix (as `address_columns(prefix)` lists
/// them), or `None` when they're NULL. Check constraints keep the required columns all
/// set or all NULL, so the last name stands for all of them.
pub(crate) fn address_from_row(row: &Row, prefix: &str) -> Result<Option<AddressParts>, RepoError> {
    let [
        first_name,
        last_name,
        company,
        line1,
        line2,
        city,
        province,
        postal_code,
        country,
        phone,
    ] = ADDRESS_FIELDS.map(|f| format!("{prefix}{f}"));
    let Some(last_name) = col::<Option<String>>(row, &last_name)? else {
        return Ok(None);
    };
    Ok(Some(AddressParts {
        first_name: col(row, &first_name)?,
        last_name,
        company: col(row, &company)?,
        line1: col(row, &line1)?,
        line2: col(row, &line2)?,
        city: col(row, &city)?,
        province: col(row, &province)?,
        postal_code: col(row, &postal_code)?,
        country: col(row, &country)?,
        phone: col(row, &phone)?,
    }))
}

/// Writes an aggregate's stamps (design note 0016, Q3) on the row its write just
/// inserted or updated, in the same transaction: `UPDATE table SET created_at, updated_at
/// WHERE key = id`. Adapters stamp every aggregate before taking its snapshot, so `None`
/// means a caller skipped that, and writes nothing.
pub(crate) async fn write_stamps(
    tx: &impl crate::store::Queries,
    table: &str,
    key: &str,
    id: &(dyn ToSql + Sync),
    id_type: Type,
    stamps: Option<Stamps>,
) -> Result<(), RepoError> {
    let Some(stamps) = stamps else {
        return Ok(());
    };
    tx.query_typed(
        &format!(
            "UPDATE {table} SET created_at = {}, updated_at = {} WHERE {key} = $1",
            ts(2),
            ts(3)
        ),
        &[
            (id, id_type),
            (&stamps.created_at.unix_micros(), Type::INT8),
            (&stamps.updated_at.unix_micros(), Type::INT8),
        ],
    )
    .await
    .map_err(crate::error::repo_error)?;
    Ok(())
}

/// The `created_at` and `updated_at` a SELECT list read with `micros`, as `Stamps`.
pub(crate) fn stamps_from_row(row: &Row) -> Result<Option<Stamps>, RepoError> {
    Ok(Some(Stamps {
        created_at: Timestamp::from_unix_micros(col(row, "created_at")?),
        updated_at: Timestamp::from_unix_micros(col(row, "updated_at")?),
    }))
}

/// `created_at, updated_at` as microseconds, for a SELECT list (`stamps_from_row`
/// reads them back). `prefix` qualifies them (`"o."`), or is empty.
pub(crate) fn stamp_columns(prefix: &str) -> String {
    format!(
        "{}, {}",
        micros(&format!("{prefix}created_at")),
        micros(&format!("{prefix}updated_at"))
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn micros_names_the_column_without_its_table() {
        assert_eq!(
            micros("o.placed_at"),
            "(extract(epoch FROM o.placed_at) * 1000000)::bigint AS placed_at"
        );
    }

    #[test]
    fn micros_names_an_unqualified_column_after_itself() {
        assert_eq!(
            micros("created_at"),
            "(extract(epoch FROM created_at) * 1000000)::bigint AS created_at"
        );
    }
}
