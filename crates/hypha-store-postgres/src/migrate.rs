use tokio_postgres::{Client, types::Type};

struct Migration {
    version: i32,
    name: &'static str,
    sql: &'static str,
}

/// Every migration, in order. Append only: never edit or reorder an applied one.
const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "products",
        sql: include_str!("../migrations/0001_products.sql"),
    },
    Migration {
        version: 2,
        name: "markets",
        sql: include_str!("../migrations/0002_markets.sql"),
    },
    Migration {
        version: 3,
        name: "carts",
        sql: include_str!("../migrations/0003_carts.sql"),
    },
];

/// Applies the migrations that haven't been applied yet, each in its own transaction.
///
/// Run it from native code (a CLI, host startup), never from a Worker. Safe to run
/// concurrently: the `schema_migrations` lock makes other runners wait.
pub async fn migrate(client: &mut Client) -> Result<(), tokio_postgres::Error> {
    client
        .batch_execute(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                version    integer     PRIMARY KEY,
                name       text        NOT NULL,
                applied_at timestamptz NOT NULL DEFAULT now()
            )",
        )
        .await?;

    for migration in MIGRATIONS {
        let tx = client.transaction().await?;
        tx.batch_execute("LOCK TABLE schema_migrations IN EXCLUSIVE MODE")
            .await?;
        let applied = tx
            .query_typed_opt(
                "SELECT 1 FROM schema_migrations WHERE version = $1",
                &[(&migration.version, Type::INT4)],
            )
            .await?
            .is_some();
        if !applied {
            tx.batch_execute(migration.sql).await?;
            tx.query_typed(
                "INSERT INTO schema_migrations (version, name) VALUES ($1, $2)",
                &[
                    (&migration.version, Type::INT4),
                    (&migration.name, Type::TEXT),
                ],
            )
            .await?;
        }
        tx.commit().await?;
    }
    Ok(())
}
