//! Multi-query loads read one consistent snapshot (review plan B3), against a real
//! Postgres. Needs `DATABASE_URL` like the contract tests; skips itself without it.
//!
//! A writer keeps moving a customer's default address to a new address and removing
//! the old one, while a reader keeps loading the customer on another connection. If the
//! load's queries saw different moments, the reader would get a default that isn't in
//! the address book, `restore` would reject it, and the load would fail with
//! `RepoError::Backend`.

use hypha_core::{Address, AddressId, AddressParts, CountryCode, CustomerId, Email, RepoError};
use hypha_customers::{Customer, CustomerRepo};
use hypha_store_postgres::{CORE, PgStore, migrate};
use tokio_postgres::{Client, NoTls};
use uuid::Uuid;

const SCHEMA: &str = "consistent_reads";
const ROUNDS: u128 = 150;

async fn connect(url: &str) -> Client {
    let (client, connection) = tokio_postgres::connect(url, NoTls)
        .await
        .expect("connect to DATABASE_URL");
    tokio::spawn(connection);
    client
        .batch_execute(&format!("SET search_path TO {SCHEMA}"))
        .await
        .expect("set search_path");
    client
}

fn address(n: u128) -> Address {
    Address::new(AddressParts {
        first_name: None,
        last_name: "Horvat".to_string(),
        company: None,
        line1: format!("Ilica {n}"),
        line2: None,
        city: "Zagreb".to_string(),
        province: None,
        postal_code: None,
        country: CountryCode::new("HR").unwrap(),
        phone: None,
    })
    .unwrap()
}

fn aid(n: u128) -> AddressId {
    AddressId::from_uuid(Uuid::from_u128(n))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_load_never_mixes_two_versions_of_a_customer() {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set: skipping the consistent read test");
        return;
    };
    let (mut setup, connection) = tokio_postgres::connect(&url, NoTls).await.unwrap();
    tokio::spawn(connection);
    setup
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {SCHEMA} CASCADE; CREATE SCHEMA {SCHEMA}; \
             SET search_path TO {SCHEMA};"
        ))
        .await
        .unwrap();
    migrate(&mut setup, &[CORE]).await.unwrap();

    let id = CustomerId::from_uuid(Uuid::from_u128(1));
    let mut customer = Customer::new(id, Email::new("ana@example.hr").unwrap());
    customer
        .add_address(aid(1), address(1), true, true)
        .unwrap();
    PgStore::new(setup).insert(&mut customer).await.unwrap();

    let writer = PgStore::new(connect(&url).await);
    let reader = PgStore::new(connect(&url).await);

    let write = tokio::spawn(async move {
        for n in 2..=ROUNDS {
            let mut c = writer.get(id).await.unwrap().unwrap();
            c.add_address(aid(n), address(n), true, true).unwrap();
            c.remove_address(aid(n - 1)).unwrap();
            writer.update(&mut c).await.unwrap();
        }
    });
    let read = tokio::spawn(async move {
        let mut loads = 0;
        while !write_done(&reader).await {
            match reader.get(id).await {
                Ok(Some(_)) => loads += 1,
                Err(RepoError::Backend(e)) => panic!("load mixed two versions: {e}"),
                other => panic!("unexpected: {other:?}"),
            }
        }
        loads
    });

    write.await.unwrap();
    let loads = read.await.unwrap();
    assert!(loads > 0, "the reader ran concurrently with the writer");
}

/// The writer is done once the last address is the only one left.
async fn write_done(store: &PgStore<Client>) -> bool {
    let id = CustomerId::from_uuid(Uuid::from_u128(1));
    matches!(
        store.get(id).await,
        Ok(Some(c)) if c.default_shipping() == Some(aid(ROUNDS))
    )
}
