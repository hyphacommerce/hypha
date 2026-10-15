//! The migration runner against a real Postgres (needs `DATABASE_URL`, like contract.rs).

use hypha_store_postgres::{CORE, MigrateError, Migration, MigrationSet, migrate};
use tokio_postgres::{Client, NoTls};

async fn fresh_schema(schema: &str) -> Option<Client> {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set: skipping the migration tests");
        return None;
    };
    let (client, connection) = hypha_store_postgres::database_config(&url)
        .expect("DATABASE_URL")
        .connect(NoTls)
        .await
        .expect("connect");
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
    let (mut second, connection) = hypha_store_postgres::database_config(&url)
        .expect("DATABASE_URL")
        .connect(NoTls)
        .await
        .unwrap();
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

#[tokio::test]
async fn edited_sql_is_an_error_and_old_rows_get_a_checksum() {
    let Some(mut client) = fresh_schema("migrate_checksum").await else {
        return;
    };
    migrate(&mut client, &[CORE]).await.unwrap();

    // Rows from before checksums: the next run fills them in instead of failing.
    client
        .batch_execute("UPDATE schema_migrations SET checksum = NULL")
        .await
        .unwrap();
    migrate(&mut client, &[CORE]).await.unwrap();
    let missing: i64 = client
        .query_one(
            "SELECT count(*) FROM schema_migrations WHERE checksum IS NULL",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(missing, 0);

    // What an edited migration file looks like from the database's side.
    client
        .batch_execute("UPDATE schema_migrations SET checksum = 'edited' WHERE version = 3")
        .await
        .unwrap();
    let result = migrate(&mut client, &[CORE]).await;
    assert!(
        matches!(&result, Err(MigrateError::Edited { module, version: 3 }) if module == "core"),
        "{result:?}"
    );
}

#[tokio::test]
async fn a_database_from_a_newer_hypha_is_refused() {
    let Some(mut client) = fresh_schema("migrate_unknown").await else {
        return;
    };
    migrate(&mut client, &[CORE]).await.unwrap();
    client
        .batch_execute(
            "INSERT INTO schema_migrations (module, version, name, checksum)
             VALUES ('core', 999, 'from_the_future', 'x')",
        )
        .await
        .unwrap();

    let result = migrate(&mut client, &[CORE]).await;
    assert!(
        matches!(&result, Err(MigrateError::Unknown { module, version: 999 }) if module == "core"),
        "{result:?}"
    );
}

/// Core up to migration 0018, before the product editing rules (design note 0014).
const BEFORE_PRODUCT_EDITING: MigrationSet = MigrationSet {
    module: "core",
    migrations: CORE.migrations.split_at(18).0,
};

/// A product with one option (`name`, one value `label`) and one variant per SKU, all
/// selecting that value, written the way rows were before 0019 (no checks then).
async fn old_product(client: &Client, n: u32, name: &str, label: &str, skus: &[Option<&str>]) {
    let product = format!("00000000-0000-0000-0000-{n:012}");
    let option = format!("00000000-0000-0000-0001-{n:012}");
    let value = format!("00000000-0000-0000-0002-{n:012}");
    client
        .execute(
            "INSERT INTO products (id, version, handle, title, status)
             VALUES ($1::text::uuid, 1, 'p' || $2::text, 'Product', 'draft')",
            &[&product, &n.to_string()],
        )
        .await
        .unwrap();
    client
        .execute(
            "INSERT INTO product_options (id, product_id, position, name)
             VALUES ($1::text::uuid, $2::text::uuid, 0, $3)",
            &[&option, &product, &name],
        )
        .await
        .unwrap();
    client
        .execute(
            "INSERT INTO product_option_values (id, option_id, position, label)
             VALUES ($1::text::uuid, $2::text::uuid, 0, $3)",
            &[&value, &option, &label],
        )
        .await
        .unwrap();
    for (position, sku) in skus.iter().enumerate() {
        client
            .execute(
                "INSERT INTO variants (id, product_id, position, sku, price_amount, price_currency, selections)
                 VALUES (gen_random_uuid(), $1::text::uuid, $2, $3, 100, 'EUR', ARRAY[$4::text::uuid])",
                &[&product, &(position as i32), sku, &value],
            )
            .await
            .unwrap();
    }
}

/// Migrates rows written before 0019; the error text, if 0019 refused them.
async fn migrate_old(schema: &str, rows: impl AsyncFnOnce(&Client)) -> Option<Result<(), String>> {
    let mut client = fresh_schema(schema).await?;
    migrate(&mut client, &[BEFORE_PRODUCT_EDITING])
        .await
        .unwrap();
    rows(&client).await;
    Some(
        migrate(&mut client, &[CORE])
            .await
            .map_err(|e| e.to_string()),
    )
}

#[tokio::test]
async fn product_editing_accepts_rows_that_follow_its_rules_and_trims_skus() {
    let Some(result) = migrate_old("migrate_0019_ok", async |c| {
        old_product(c, 1, "Size", "S", &[Some(" SKU-1\u{a0}"), Some("  ")]).await;
    })
    .await
    else {
        return;
    };
    assert_eq!(result, Ok(()));
}

#[tokio::test]
async fn product_editing_refuses_rows_that_would_stop_loading() {
    let blank_name = migrate_old("migrate_0019_blank", async |c| {
        old_product(c, 1, "\u{a0}\u{3000}", "S", &[None]).await;
    });
    let Some(result) = blank_name.await else {
        return;
    };
    assert!(
        result
            .unwrap_err()
            .contains("00000000-0000-0000-0000-000000000001")
    );

    // Equal once trimmed the way Rust trims (a no-break space).
    let same_sku = migrate_old("migrate_0019_sku", async |c| {
        old_product(c, 1, "Size", "S", &[Some("ABC")]).await;
        old_product(c, 2, "Size", "S", &[Some("ABC\u{a0}")]).await;
    });
    assert!(same_sku.await.unwrap().is_err());

    let too_many = migrate_old("migrate_0019_variants", async |c| {
        old_product(c, 1, "Size", "S", &[None; 2049]).await;
    });
    assert!(
        too_many
            .await
            .unwrap()
            .unwrap_err()
            .contains("000000000001")
    );
}
