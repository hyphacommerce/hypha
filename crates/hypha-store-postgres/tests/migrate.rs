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

/// Core before sales channels (design note 0020): migrations 0001 to 0035.
const BEFORE_CHANNELS: MigrationSet = MigrationSet {
    module: "core",
    migrations: CORE.migrations.split_at(35).0,
};

#[tokio::test]
async fn existing_products_stay_sold_in_the_online_store() {
    let Some(mut client) = fresh_schema("migrate_0036_channels").await else {
        return;
    };
    migrate(&mut client, &[BEFORE_CHANNELS]).await.unwrap();
    old_product(&client, 1, "Size", "S", &[None]).await;
    old_product(&client, 2, "Size", "M", &[None]).await;
    migrate(&mut client, &[CORE]).await.unwrap();

    let channels = client
        .query("SELECT name, kind FROM channels", &[])
        .await
        .unwrap();
    assert_eq!(channels.len(), 1);
    assert_eq!(
        (
            channels[0].get::<_, String>(0),
            channels[0].get::<_, String>(1)
        ),
        ("Online Store".to_string(), "online_store".to_string())
    );
    let published: i64 = client
        .query_one("SELECT count(*) FROM channel_products", &[])
        .await
        .unwrap()
        .get(0);
    assert_eq!(published, 2);
}

/// Core before the ISO tables (design note 0021): migrations 0001 to 0039.
const BEFORE_REFERENCE_CODES: MigrationSet = MigrationSet {
    module: "core",
    migrations: CORE.migrations.split_at(39).0,
};

#[tokio::test]
async fn reference_codes_name_every_stored_code_core_no_longer_reads() {
    let Some(mut client) = fresh_schema("migrate_0040_codes").await else {
        return;
    };
    migrate(&mut client, &[BEFORE_REFERENCE_CODES])
        .await
        .unwrap();
    // Any capital letters passed 0011's checks: these are a metal and an unassigned code.
    client
        .batch_execute(
            "INSERT INTO markets (id, version, name, currency)
                 VALUES ('00000000-0000-0000-0000-000000000001', 1, 'Gold', 'XAU');
             INSERT INTO market_countries (market_id, position, country)
                 VALUES ('00000000-0000-0000-0000-000000000001', 0, 'XK'),
                        ('00000000-0000-0000-0000-000000000001', 1, 'HR');
             INSERT INTO tax_regions (id, version, country, default_rate)
                 VALUES ('00000000-0000-0000-0000-000000000002', 1, 'XK', 0);",
        )
        .await
        .unwrap();

    let error = migrate(&mut client, &[CORE]).await.unwrap_err().to_string();
    assert!(
        error.contains(
            "market_countries.country 'XK'; markets.currency 'XAU'; tax_regions.country 'XK'"
        ),
        "{error}"
    );
    assert!(!error.contains("'HR'"), "{error}");
}

/// 0040's lists, read from the SQL: `name text[] := ARRAY[ 'A', 'B', … ];`.
fn listed_in_0040(name: &str) -> Vec<String> {
    let sql = include_str!("../migrations/0040_reference_codes.sql");
    let opening = format!("{name} text[] := ARRAY[");
    let list = &sql[sql.find(&opening).unwrap() + opening.len()..];
    let list = &list[..list.find(']').unwrap()];
    list.split(',')
        .map(|code| code.trim().trim_matches('\'').to_string())
        .collect()
}

/// Removing a code from core's tables stops rows that use it from loading, so it needs a
/// new migration that checks for it (0040's lists stay as they were applied).
#[test]
fn every_code_0040_accepts_is_still_in_core() {
    let currencies = listed_in_0040("currencies");
    let countries = listed_in_0040("countries");
    assert_eq!((currencies.len(), countries.len()), (155, 249));
    for code in &currencies {
        assert!(hypha_core::Currency::new(code).is_ok(), "{code}");
    }
    for code in &countries {
        assert!(hypha_core::CountryCode::new(code).is_ok(), "{code}");
    }
}

/// Core before payments (design note 0027): migrations 0001 to 0046.
const BEFORE_PAYMENTS: MigrationSet = MigrationSet {
    module: "core",
    migrations: CORE.migrations.split_at(46).0,
};

#[tokio::test]
async fn orders_marked_paid_before_payments_get_their_payment() {
    let Some(mut client) = fresh_schema("migrate_0047_payments").await else {
        return;
    };
    migrate(&mut client, &[BEFORE_PAYMENTS]).await.unwrap();
    for (n, status) in [(1, "paid"), (2, "pending")] {
        client
            .batch_execute(&format!(
                "INSERT INTO orders (id, version, number, placed_at, cart_id, market_id,
                     currency, country, taxes_included, customer_id, email, subtotal,
                     tax_total, total, financial_status, fulfillment_status, channel_id)
                 VALUES ('00000000-0000-0000-0000-00000000000{n}', 2, 100{n}, now(),
                     gen_random_uuid(), gen_random_uuid(), 'EUR', 'HR', true,
                     gen_random_uuid(), 'ana@example.hr', 1250, 0, 1250, '{status}',
                     'unfulfilled', gen_random_uuid())"
            ))
            .await
            .unwrap();
    }
    // Marked paid a while ago: the payment is of that moment.
    client
        .batch_execute(
            "UPDATE orders SET updated_at = '2025-03-01 10:20:30.456789+00'
             WHERE id = '00000000-0000-0000-0000-000000000001'",
        )
        .await
        .unwrap();
    migrate(&mut client, &[CORE]).await.unwrap();

    let payments = client
        .query(
            "SELECT order_id::text, provider, currency, amount, status, paid_at IS NOT NULL,
                 method_id IS NULL
             FROM payments",
            &[],
        )
        .await
        .unwrap();
    assert_eq!(payments.len(), 1, "only the paid order");
    let p = &payments[0];
    assert_eq!(
        (
            p.get::<_, String>(0),
            p.get::<_, String>(1),
            p.get::<_, String>(2),
            p.get::<_, i64>(3),
            p.get::<_, String>(4),
            p.get::<_, bool>(5),
            p.get::<_, bool>(6),
        ),
        (
            "00000000-0000-0000-0000-000000000001".to_string(),
            "manual".to_string(),
            "EUR".to_string(),
            1250,
            "paid".to_string(),
            true,
            true
        )
    );

    // A UUIDv7 of when the order was marked paid, so it sorts like the IDs hosts make.
    let row = client
        .query_one(
            "SELECT payments.id, payments.paid_at = o.updated_at, payments.created_at = o.updated_at,
                 payments.updated_at = o.updated_at
             FROM payments JOIN orders o ON o.id = payments.order_id",
            &[],
        )
        .await
        .unwrap();
    let id: uuid::Uuid = row.get(0);
    assert_eq!(id.get_version_num(), 7);
    assert_eq!(id.get_variant(), uuid::Variant::RFC4122);
    let bytes = id.as_bytes();
    let mut millis = [0u8; 8];
    millis[2..].copy_from_slice(&bytes[..6]);
    assert_eq!(u64::from_be_bytes(millis), 1_740_824_430_456);
    assert_eq!(
        (
            row.get::<_, bool>(1),
            row.get::<_, bool>(2),
            row.get::<_, bool>(3)
        ),
        (true, true, true)
    );
    // The smallest UUIDv7 of now sorts after it, in Rust and in Postgres alike.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let mut fresh = [0u8; 16];
    fresh[..6].copy_from_slice(&now.to_be_bytes()[2..]);
    fresh[6] = 0x70;
    fresh[8] = 0x80;
    let fresh = uuid::Uuid::from_bytes(fresh);
    assert!(id < fresh);
    let below: bool = client
        .query_one("SELECT $1::uuid < $2::uuid", &[&id, &fresh])
        .await
        .unwrap()
        .get(0);
    assert!(below);
}

// ---------- what a spore's migrations may do (design note 0030, §5) ----------

/// A spore that keeps to its own: tables, an index, a sequence, a function, a trigger on
/// its table, and keys into core's that cascade or set null.
const WELL_BEHAVED: MigrationSet = MigrationSet {
    module: "spore_tidy",
    migrations: &[Migration {
        version: 1,
        name: "own",
        sql: "CREATE TABLE spore_tidy_notes (
                  id uuid PRIMARY KEY,
                  product_id uuid REFERENCES products (id) ON DELETE CASCADE,
                  store_id uuid REFERENCES stores (id) ON DELETE SET NULL,
                  body text NOT NULL
              );
              CREATE INDEX spore_tidy_notes_product ON spore_tidy_notes (product_id);
              CREATE SEQUENCE spore_tidy_seq;
              CREATE FUNCTION spore_tidy_touch() RETURNS trigger LANGUAGE plpgsql
                  AS $$ BEGIN RETURN NEW; END $$;
              CREATE TRIGGER touch BEFORE UPDATE ON spore_tidy_notes
                  FOR EACH ROW EXECUTE FUNCTION spore_tidy_touch();",
    }],
};

#[tokio::test]
async fn a_spore_that_keeps_to_its_own_tables_migrates() {
    let Some(mut client) = fresh_schema("migrate_spore_tidy").await else {
        return;
    };
    migrate(&mut client, &[CORE, WELL_BEHAVED]).await.unwrap();
    client
        .query_typed("SELECT count(*) FROM spore_tidy_notes", &[])
        .await
        .expect("the spore's table exists");
}

/// A spore set of one migration, `sql`.
fn spore(sql: &'static str) -> MigrationSet {
    MigrationSet {
        module: "spore_rogue",
        migrations: Box::leak(Box::new([Migration {
            version: 1,
            name: "rogue",
            sql,
        }])),
    }
}

/// Migrates core, then `sql` as a spore's: the problems reported, and whether the
/// migration was left unapplied.
async fn refused(schema: &str, sql: &'static str) -> Option<Vec<String>> {
    let mut client = fresh_schema(schema).await?;
    migrate(&mut client, &[CORE]).await.unwrap();
    let problems = match migrate(&mut client, &[CORE, spore(sql)]).await {
        Err(MigrateError::SporeObjects {
            module,
            version,
            problems,
        }) => {
            assert_eq!((module.as_str(), version), ("spore_rogue", 1));
            problems
        }
        other => panic!("expected the spore's migration refused, got {other:?}"),
    };
    // Rolled back: nothing of it stays, and it isn't recorded.
    assert_eq!(applied(&client).await, core_rows());
    Some(problems)
}

#[tokio::test]
async fn a_spore_table_without_its_prefix_is_refused() {
    let Some(problems) = refused(
        "migrate_rogue_name",
        "CREATE TABLE notes (id uuid PRIMARY KEY)",
    )
    .await
    else {
        return;
    };
    assert!(
        problems.contains(&"table \"notes\" is not named spore_rogue_*".to_string()),
        "{problems:?}"
    );
}

#[tokio::test]
async fn a_spore_key_that_could_block_a_core_delete_is_refused() {
    let Some(problems) = refused(
        "migrate_rogue_fk",
        "CREATE TABLE spore_rogue_notes (
             id uuid PRIMARY KEY,
             product_id uuid REFERENCES products (id)
         )",
    )
    .await
    else {
        return;
    };
    assert_eq!(
        problems,
        [
            "foreign key \"spore_rogue_notes_product_id_fkey\" on \"spore_rogue_notes\" references \"products\" without ON DELETE CASCADE or SET NULL"
        ]
    );
}

#[tokio::test]
async fn a_spore_trigger_or_column_on_a_core_table_is_refused() {
    let Some(problems) = refused(
        "migrate_rogue_core",
        "CREATE FUNCTION spore_rogue_audit() RETURNS trigger LANGUAGE plpgsql
             AS $$ BEGIN RETURN NEW; END $$;
         CREATE TRIGGER spore_rogue_audit AFTER INSERT ON products
             FOR EACH ROW EXECUTE FUNCTION spore_rogue_audit();
         ALTER TABLE products ADD COLUMN spore_rogue_rating integer;",
    )
    .await
    else {
        return;
    };
    assert_eq!(
        problems,
        [
            "column \"spore_rogue_rating\" was added to \"products\", a table that isn't the spore's",
            "trigger \"spore_rogue_audit\" was added to \"products\", a table that isn't the spore's",
        ]
    );
}

#[tokio::test]
async fn a_spore_dropping_a_core_table_is_refused() {
    let Some(problems) = refused("migrate_rogue_drop", "DROP TABLE idempotency_keys").await else {
        return;
    };
    assert!(
        problems.contains(
            &"table \"idempotency_keys\" was dropped, and it isn't the spore's".to_string()
        ),
        "{problems:?}"
    );
}
