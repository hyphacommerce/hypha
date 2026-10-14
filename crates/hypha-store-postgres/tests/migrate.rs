//! The migration runner against a real Postgres (needs `DATABASE_URL`, like contract.rs).

use hypha_store_postgres::{CORE, MigrateError, Migration, MigrationSet, migrate};
use tokio_postgres::{Client, NoTls};

async fn fresh_schema(schema: &str) -> Option<Client> {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set: skipping the migration tests");
        return None;
    };
    let (client, connection) = tokio_postgres::connect(&url, NoTls).await.expect("connect");
    tokio::spawn(connection);
    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {schema} CASCADE; CREATE SCHEMA {schema}; \
             SET search_path TO {schema};"
        ))
        .await
        .expect("reset schema");
    Some(client)
}

async fn applied(client: &Client) -> Vec<(String, i32, String)> {
    client
        .query_typed(
            "SELECT module, version, name FROM schema_migrations ORDER BY module, version",
            &[],
        )
        .await
        .expect("read schema_migrations")
        .iter()
        .map(|r| (r.get(0), r.get(1), r.get(2)))
        .collect()
}

fn core_rows() -> Vec<(String, i32, String)> {
    CORE.migrations
        .iter()
        .map(|m| ("core".to_string(), m.version, m.name.to_string()))
        .collect()
}

const SPORE: MigrationSet = MigrationSet {
    module: "spore_test",
    migrations: &[Migration {
        version: 1,
        name: "notes",
        sql: "CREATE TABLE spore_test_notes (id uuid PRIMARY KEY, product_id uuid NOT NULL)",
    }],
};

#[tokio::test]
async fn running_twice_applies_each_migration_once() {
    let Some(mut client) = fresh_schema("migrate_twice").await else {
        return;
    };
    migrate(&mut client, &[CORE]).await.unwrap();
    migrate(&mut client, &[CORE]).await.unwrap();
    assert_eq!(applied(&client).await, core_rows());
}

#[tokio::test]
async fn spore_versions_are_independent_of_core() {
    let Some(mut client) = fresh_schema("migrate_spore").await else {
        return;
    };
    migrate(&mut client, &[CORE, SPORE]).await.unwrap();

    let mut expected = core_rows();
    expected.push(("spore_test".to_string(), 1, "notes".to_string()));
    assert_eq!(applied(&client).await, expected);
    client
        .query_typed("SELECT count(*) FROM spore_test_notes", &[])
        .await
        .expect("the spore's table exists");
}

#[tokio::test]
async fn upgrades_a_table_from_before_modules() {
    let Some(mut client) = fresh_schema("migrate_upgrade").await else {
        return;
    };
    // The old shape: version alone was the key; all rows were core's.
    for m in &CORE.migrations[..3] {
        client.batch_execute(m.sql).await.unwrap();
    }
    client
        .batch_execute(
            "CREATE TABLE schema_migrations (version integer PRIMARY KEY, name text NOT NULL,
                applied_at timestamptz NOT NULL DEFAULT now());
             INSERT INTO schema_migrations (version, name)
                VALUES (1, 'products'), (2, 'markets'), (3, 'carts');",
        )
        .await
        .unwrap();

    // Core is already "applied", so only the spore runs; the old rows become core's.
    migrate(&mut client, &[CORE, SPORE]).await.unwrap();

    let mut expected = core_rows();
    expected.push(("spore_test".to_string(), 1, "notes".to_string()));
    assert_eq!(applied(&client).await, expected);
}

#[tokio::test]
async fn an_edited_migration_is_an_error() {
    let Some(mut client) = fresh_schema("migrate_mismatch").await else {
        return;
    };
    migrate(&mut client, &[CORE]).await.unwrap();
    client
        .batch_execute("UPDATE schema_migrations SET name = 'renamed' WHERE version = 2")
        .await
        .unwrap();

    let result = migrate(&mut client, &[CORE]).await;
    assert!(
        matches!(&result, Err(MigrateError::Mismatch { module, version: 2, applied, .. })
            if module == "core" && applied == "renamed"),
        "{result:?}"
    );
}

/// Review plan B8: two hosts starting at once used to race on creating
/// `schema_migrations` before any lock was taken.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_runners_starting_at_once_both_succeed() {
    let Some(mut first) = fresh_schema("migrate_concurrent").await else {
        return;
    };
    let url = std::env::var("DATABASE_URL").unwrap();
    let (mut second, connection) = tokio_postgres::connect(&url, NoTls).await.unwrap();
    tokio::spawn(connection);
    second
        .batch_execute("SET search_path TO migrate_concurrent")
        .await
        .unwrap();

    let (a, b) = tokio::join!(migrate(&mut first, &[CORE]), migrate(&mut second, &[CORE]));

    a.unwrap();
    b.unwrap();
    assert_eq!(applied(&first).await, core_rows());
}

/// The schema rejects what `restore` would, even from a manual edit.
#[tokio::test]
async fn checks_reject_data_the_domain_never_writes() {
    let Some(mut client) = fresh_schema("migrate_checks").await else {
        return;
    };
    migrate(&mut client, &[CORE]).await.unwrap();
    client
        .batch_execute(
            "INSERT INTO markets (id, version, name, currency)
             VALUES ('00000000-0000-0000-0000-000000000001', 1, 'EU', 'EUR')",
        )
        .await
        .unwrap();

    for bad in [
        "INSERT INTO markets (id, version, name, currency)
         VALUES ('00000000-0000-0000-0000-000000000002', 1, 'EU', 'eur')",
        "INSERT INTO market_countries (country, market_id, position)
         VALUES ('hr', '00000000-0000-0000-0000-000000000001', 0)",
        "INSERT INTO customers (id, version, email) VALUES
         ('00000000-0000-0000-0000-000000000003', 1, 'Ana@example.hr')",
    ] {
        let err = client.batch_execute(bad).await.unwrap_err();
        let code = err.as_db_error().map(|e| e.code().clone());
        assert_eq!(
            code,
            Some(tokio_postgres::error::SqlState::CHECK_VIOLATION),
            "{bad}"
        );
    }
}
