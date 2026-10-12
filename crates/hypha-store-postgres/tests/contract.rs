//! Runs the shared repository contract (`hypha_testkit::contract`) against a real
//! Postgres. Needs `DATABASE_URL`, e.g. the database from the repo's `compose.yaml`:
//!
//!   docker compose up -d
//!   DATABASE_URL=postgres://hypha:hypha@localhost:5432/hypha cargo test -p hypha-store-postgres
//!
//! Without `DATABASE_URL` the tests print a notice and pass, so `cargo test --workspace`
//! still works on machines without Docker.

use hypha_store_postgres::{PgStore, migrate};
use hypha_testkit::contract;
use tokio_postgres::{Client, NoTls};

fn database_url() -> Option<String> {
    let url = std::env::var("DATABASE_URL").ok();
    if url.is_none() {
        eprintln!("DATABASE_URL not set: skipping the Postgres contract tests");
    }
    url
}

/// A store on a fresh, fully migrated schema. Each test uses its own schema name, so
/// tests running in parallel never see each other's rows.
async fn fresh_store(url: &str, schema: &str) -> PgStore<Client> {
    let (mut client, connection) = tokio_postgres::connect(url, NoTls)
        .await
        .expect("connect to DATABASE_URL");
    tokio::spawn(connection);
    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {schema} CASCADE; CREATE SCHEMA {schema}; \
             SET search_path TO {schema};"
        ))
        .await
        .expect("reset schema");
    migrate(&mut client).await.expect("migrate");
    PgStore::new(client)
}

#[tokio::test]
async fn product_repo() {
    let Some(url) = database_url() else { return };
    contract::product_repo(async || fresh_store(&url, "contract_products").await).await;
}
