use hypha_core::{
    Address, AddressId, AddressParts, CustomerId, Email, RepoError, Version, Versioned,
};
use hypha_customers::{Customer, CustomerRepo, CustomerSnapshot, SavedAddress};
use tokio_postgres::{GenericClient, Row, types::Type};
use uuid::Uuid;

use crate::{
    columns::{address_columns, address_from_row, version_from_db, version_to_db},
    error::{corrupt, repo_error},
    store::{PgConnection, PgStore, consistent_read, in_transaction},
};

impl<C: PgConnection> CustomerRepo for PgStore<C> {
    async fn get(&self, id: CustomerId) -> Result<Option<Customer>, RepoError> {
        let mut conn = self.conn().await;
        consistent_read!(conn.client(), |tx| {
            load(tx, "id = $1", &id.as_uuid(), Type::UUID).await
        })
    }

    async fn get_by_email(&self, email: &Email) -> Result<Option<Customer>, RepoError> {
        let mut conn = self.conn().await;
        consistent_read!(conn.client(), |tx| {
            load(tx, "email = $1", &email.as_str(), Type::TEXT).await
        })
    }

    async fn insert(&self, customer: &mut Customer) -> Result<(), RepoError> {
        let s = customer.snapshot();
        in_transaction!(self, |tx| insert_customer(tx, &s).await)?;
        customer.set_version(Version::NEW.next());
        Ok(())
    }

    async fn update(&self, customer: &mut Customer) -> Result<(), RepoError> {
        let s = customer.snapshot();
        let version = in_transaction!(self, |tx| update_customer(tx, &s).await)?;
        customer.set_version(version);
        Ok(())
    }
}

/// `CustomerRepo::update` inside the caller's transaction; the new version. The version
/// check comes first; the addresses are only replaced if it passes.
pub(crate) async fn update_customer(
    tx: &impl GenericClient,
    s: &CustomerSnapshot,
) -> Result<Version, RepoError> {
    let bumped = tx
        .query_typed_opt(
            "UPDATE customers SET version = version + 1, email = $3, first_name = $4,
                 last_name = $5, phone = $6, default_shipping_address_id = $7,
                 default_billing_address_id = $8
             WHERE id = $1 AND version = $2 RETURNING version",
            &[
                (&s.id.as_uuid(), Type::UUID),
                (&version_to_db(s.version)?, Type::INT8),
                (&s.email.as_str(), Type::TEXT),
                (&s.first_name, Type::TEXT),
                (&s.last_name, Type::TEXT),
                (&s.phone, Type::TEXT),
                (&s.default_shipping.map(|a| a.as_uuid()), Type::UUID),
                (&s.default_billing.map(|a| a.as_uuid()), Type::UUID),
            ],
        )
        .await
        .map_err(repo_error)?;
    let row = bumped.ok_or(RepoError::StaleVersion)?;
    tx.query_typed(
        "DELETE FROM customer_addresses WHERE customer_id = $1",
        &[(&s.id.as_uuid(), Type::UUID)],
    )
    .await
    .map_err(repo_error)?;
    write_addresses(tx, s.id, &s.addresses).await?;
    version_from_db(row.get(0))
}

/// `CustomerRepo::insert` inside the caller's transaction (also used by `PgBatch`).
pub(crate) async fn insert_customer(
    tx: &impl GenericClient,
    s: &CustomerSnapshot,
) -> Result<(), RepoError> {
    tx.query_typed(
        "INSERT INTO customers (id, version, email, first_name, last_name, phone,
             default_shipping_address_id, default_billing_address_id)
         VALUES ($1, 1, $2, $3, $4, $5, $6, $7)",
        &[
            (&s.id.as_uuid(), Type::UUID),
            (&s.email.as_str(), Type::TEXT),
            (&s.first_name, Type::TEXT),
            (&s.last_name, Type::TEXT),
            (&s.phone, Type::TEXT),
            (&s.default_shipping.map(|a| a.as_uuid()), Type::UUID),
            (&s.default_billing.map(|a| a.as_uuid()), Type::UUID),
        ],
    )
    .await
    .map_err(repo_error)?;
    write_addresses(tx, s.id, &s.addresses).await
}

/// All addresses in one statement; `WITH ORDINALITY` keeps their order.
async fn write_addresses(
    tx: &impl GenericClient,
    customer: CustomerId,
    addresses: &[SavedAddress],
) -> Result<(), RepoError> {
    let ids: Vec<Uuid> = addresses.iter().map(|a| a.id.as_uuid()).collect();
    let column = |f: fn(&AddressParts) -> Option<&str>| -> Vec<Option<&str>> {
        addresses.iter().map(|a| f(a.address.parts())).collect()
    };
    let first_names = column(|p| p.first_name.as_deref());
    let last_names = column(|p| Some(&p.last_name));
    let companies = column(|p| p.company.as_deref());
    let line1s = column(|p| Some(&p.line1));
    let line2s = column(|p| p.line2.as_deref());
    let cities = column(|p| Some(&p.city));
    let provinces = column(|p| p.province.as_deref());
    let postal_codes = column(|p| p.postal_code.as_deref());
    let countries = column(|p| Some(p.country.as_str()));
    let phones = column(|p| p.phone.as_deref());
    tx.query_typed(
        "INSERT INTO customer_addresses (customer_id, position, id, first_name, last_name,
             company, line1, line2, city, province, postal_code, country, phone)
         SELECT $1, (n - 1)::integer, id, f, l, co, l1, l2, ci, pr, pc, cc, ph
         FROM unnest($2::uuid[], $3::text[], $4::text[], $5::text[], $6::text[], $7::text[],
                     $8::text[], $9::text[], $10::text[], $11::text[], $12::text[])
             WITH ORDINALITY AS t (id, f, l, co, l1, l2, ci, pr, pc, cc, ph, n)",
        &[
            (&customer.as_uuid(), Type::UUID),
            (&ids, Type::UUID_ARRAY),
            (&first_names, Type::TEXT_ARRAY),
            (&last_names, Type::TEXT_ARRAY),
            (&companies, Type::TEXT_ARRAY),
            (&line1s, Type::TEXT_ARRAY),
            (&line2s, Type::TEXT_ARRAY),
            (&cities, Type::TEXT_ARRAY),
            (&provinces, Type::TEXT_ARRAY),
            (&postal_codes, Type::TEXT_ARRAY),
            (&countries, Type::TEXT_ARRAY),
            (&phones, Type::TEXT_ARRAY),
        ],
    )
    .await
    .map_err(repo_error)?;
    Ok(())
}

/// One customer found by `filter` (a condition on `$1`), with its addresses, in 2 queries.
async fn load<T: tokio_postgres::types::ToSql + Sync>(
    client: &impl GenericClient,
    filter: &'static str,
    value: &T,
    value_type: Type,
) -> Result<Option<Customer>, RepoError> {
    let Some(row) = client
        .query_typed_opt(
            &format!(
                "SELECT id, version, email, first_name, last_name, phone,
                     default_shipping_address_id, default_billing_address_id
                 FROM customers WHERE {filter}"
            ),
            &[(value, value_type)],
        )
        .await
        .map_err(repo_error)?
    else {
        return Ok(None);
    };
    let id: Uuid = row.get(0);
    let addresses = client
        .query_typed(
            &format!(
                "SELECT id, {} FROM customer_addresses
                 WHERE customer_id = $1 ORDER BY position",
                address_columns(""),
            ),
            &[(&id, Type::UUID)],
        )
        .await
        .map_err(repo_error)?
        .iter()
        .map(saved_address)
        .collect::<Result<Vec<_>, RepoError>>()?;
    let email: String = row.get(2);
    let snapshot = CustomerSnapshot {
        id: CustomerId::from_uuid(id),
        version: version_from_db(row.get(1))?,
        email: Email::new(&email).map_err(corrupt)?,
        first_name: row.get(3),
        last_name: row.get(4),
        phone: row.get(5),
        addresses,
        default_shipping: row.get::<_, Option<Uuid>>(6).map(AddressId::from_uuid),
        default_billing: row.get::<_, Option<Uuid>>(7).map(AddressId::from_uuid),
    };
    Customer::restore(snapshot)
        .map(Some)
        .map_err(|e| corrupt(format!("customer {id}: {e}")))
}

fn saved_address(row: &Row) -> Result<SavedAddress, RepoError> {
    let parts = address_from_row(row, 1).ok_or_else(|| corrupt("address without a last name"))?;
    let address = Address::new(parts).map_err(corrupt)?;
    Ok(SavedAddress {
        id: AddressId::from_uuid(row.get(0)),
        address,
    })
}
