use std::{collections::HashSet, fmt};

use tokio_postgres::{Client, Transaction, types::Type};

/// One schema change. Append only: once applied anywhere, never edit, rename or reorder it.
pub struct Migration {
    pub version: i32,
    pub name: &'static str,
    pub sql: &'static str,
}

/// The migrations of one module: Hypha's core tables, or one spore (plugin).
///
/// Versions are per module, so a spore's `version: 1` never collides with core's. Use a
/// distinct module name per crate (`"core"`, `"spore_stripe"`, ...) and prefix the spore's
/// tables the same way.
pub struct MigrationSet {
    pub module: &'static str,
    /// In strictly increasing version order.
    pub migrations: &'static [Migration],
}

/// Hypha's own tables.
pub const CORE: MigrationSet = MigrationSet {
    module: "core",
    migrations: &[
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
        Migration {
            version: 4,
            name: "market_prices_include_tax",
            sql: include_str!("../migrations/0004_market_prices_include_tax.sql"),
        },
        Migration {
            version: 5,
            name: "tax",
            sql: include_str!("../migrations/0005_tax.sql"),
        },
        Migration {
            version: 6,
            name: "cart_country",
            sql: include_str!("../migrations/0006_cart_country.sql"),
        },
        Migration {
            version: 7,
            name: "inventory",
            sql: include_str!("../migrations/0007_inventory.sql"),
        },
        Migration {
            version: 8,
            name: "customers",
            sql: include_str!("../migrations/0008_customers.sql"),
        },
        Migration {
            version: 9,
            name: "cart_buyer",
            sql: include_str!("../migrations/0009_cart_buyer.sql"),
        },
        Migration {
            version: 10,
            name: "stores",
            sql: include_str!("../migrations/0010_stores.sql"),
        },
        Migration {
            version: 11,
            name: "checks",
            sql: include_str!("../migrations/0011_checks.sql"),
        },
        Migration {
            version: 12,
            name: "idempotency_keys",
            sql: include_str!("../migrations/0012_idempotency_keys.sql"),
        },
        Migration {
            version: 13,
            name: "orders",
            sql: include_str!("../migrations/0013_orders.sql"),
        },
        Migration {
            version: 14,
            name: "cart_order",
            sql: include_str!("../migrations/0014_cart_order.sql"),
        },
        Migration {
            version: 15,
            name: "staff_auth",
            sql: include_str!("../migrations/0015_staff_auth.sql"),
        },
        Migration {
            version: 16,
            name: "password_reset_lookups",
            sql: include_str!("../migrations/0016_password_reset_lookups.sql"),
        },
        Migration {
            version: 17,
            name: "list_pages",
            sql: include_str!("../migrations/0017_list_pages.sql"),
        },
        Migration {
            version: 18,
            name: "assets",
            sql: include_str!("../migrations/0018_assets.sql"),
        },
        Migration {
            version: 19,
            name: "product_editing",
            sql: include_str!("../migrations/0019_product_editing.sql"),
        },
        Migration {
            version: 20,
            name: "asset_usages",
            sql: include_str!("../migrations/0020_asset_usages.sql"),
        },
        Migration {
            version: 21,
            name: "shipping_facts",
            sql: include_str!("../migrations/0021_shipping_facts.sql"),
        },
        Migration {
            version: 22,
            name: "shipping",
            sql: include_str!("../migrations/0022_shipping.sql"),
        },
        Migration {
            version: 23,
            name: "cart_shipping",
            sql: include_str!("../migrations/0023_cart_shipping.sql"),
        },
        Migration {
            version: 24,
            name: "order_shipping",
            sql: include_str!("../migrations/0024_order_shipping.sql"),
        },
        Migration {
            version: 25,
            name: "outbox",
            sql: include_str!("../migrations/0025_outbox.sql"),
        },
        Migration {
            version: 26,
            name: "stamps",
            sql: include_str!("../migrations/0026_stamps.sql"),
        },
        Migration {
            version: 27,
            name: "event_deliveries",
            sql: include_str!("../migrations/0027_event_deliveries.sql"),
        },
        Migration {
            version: 28,
            name: "price_list_prices_variant",
            sql: include_str!("../migrations/0028_price_list_prices_variant.sql"),
        },
        Migration {
            version: 29,
            name: "apps",
            sql: include_str!("../migrations/0029_apps.sql"),
        },
        Migration {
            version: 30,
            name: "order_fulfilment",
            sql: include_str!("../migrations/0030_order_fulfilment.sql"),
        },
    ],
};

#[derive(Debug)]
pub enum MigrateError {
    Db(tokio_postgres::Error),
    /// The migration lists themselves are wrong (duplicate module, versions out of order).
    InvalidSet(String),
    /// The database recorded a different name for a version than the code has: someone
    /// edited or reordered an applied migration.
    Mismatch {
        module: String,
        version: i32,
        applied: String,
        code: String,
    },
    /// An applied migration's SQL changed since it ran (its checksum differs).
    Edited {
        module: String,
        version: i32,
    },
    /// The database has a migration this code doesn't know: it was migrated by a newer
    /// version of the code, which this one must not run against.
    Unknown {
        module: String,
        version: i32,
    },
}

impl fmt::Display for MigrateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MigrateError::Db(e) => match e.as_db_error() {
                Some(db) => write!(f, "{db}"),
                None => write!(f, "{e}"),
            },
            MigrateError::InvalidSet(why) => write!(f, "invalid migration set: {why}"),
            MigrateError::Mismatch {
                module,
                version,
                applied,
                code,
            } => write!(
                f,
                "migration {module}/{version} was applied as {applied:?} but the code calls it \
                 {code:?}; applied migrations must never change"
            ),
            MigrateError::Edited { module, version } => write!(
                f,
                "migration {module}/{version} was edited after it was applied; applied \
                 migrations must never change (add a new one instead)"
            ),
            MigrateError::Unknown { module, version } => write!(
                f,
                "the database has migration {module}/{version}, which this code doesn't \
                 know: it was migrated by a newer Hypha"
            ),
        }
    }
}

impl std::error::Error for MigrateError {}

impl From<tokio_postgres::Error> for MigrateError {
    fn from(e: tokio_postgres::Error) -> Self {
        MigrateError::Db(e)
    }
}

/// Arbitrary, but the same for every Hypha migration runner (ASCII "hypha-mg").
const MIGRATION_LOCK: i64 = 0x6879_7068_612d_6d67;

/// Waits for, then holds until the end of `tx`, the lock every migration runner takes, so
/// concurrent runners (several hosts starting at once) apply migrations one at a time.
/// An advisory lock needs no table, so it also covers creating `schema_migrations`.
async fn lock(tx: &Transaction<'_>) -> Result<(), tokio_postgres::Error> {
    tx.query_typed(
        "SELECT pg_advisory_xact_lock($1)",
        &[(&MIGRATION_LOCK, Type::INT8)],
    )
    .await?;
    Ok(())
}

/// Checks the lists before touching the database.
fn check_sets(sets: &[MigrationSet]) -> Result<(), MigrateError> {
    let mut modules = HashSet::new();
    for set in sets {
        if !modules.insert(set.module) {
            return Err(MigrateError::InvalidSet(format!(
                "module {:?} listed twice",
                set.module
            )));
        }
        let mut previous = 0;
        for m in set.migrations {
            if m.version <= previous {
                return Err(MigrateError::InvalidSet(format!(
                    "{}: version {} is not greater than {previous}",
                    set.module, m.version
                )));
            }
            previous = m.version;
        }
    }
    Ok(())
}

/// A fingerprint of a migration's SQL, stored when it is applied so a later edit is
/// caught. FNV-1a (64-bit): it only has to notice changes, not resist an attacker, so
/// it needs no dependency. Carriage returns are skipped, so a checkout with CRLF line
/// endings (Git on Windows) hashes the same as one with LF.
fn checksum(sql: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in sql.bytes().filter(|b| *b != b'\r') {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("fnv1a64:{hash:016x}")
}

/// Applies every migration not applied yet, module by module in the given order (core
/// first, then spores), each in its own transaction.
///
/// Run it from native code (a CLI, host startup), never from a Worker. Safe to run
/// concurrently: every step takes the same advisory lock, so other runners wait.
pub async fn migrate(client: &mut Client, sets: &[MigrationSet]) -> Result<(), MigrateError> {
    check_sets(sets)?;

    // Creating or upgrading the bookkeeping table happens under the same lock as every
    // migration, so two hosts starting at once can't race on it (review plan B8).
    let tx = client.transaction().await?;
    lock(&tx).await?;
    tx.batch_execute(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
                module     text        NOT NULL,
                version    integer     NOT NULL,
                name       text        NOT NULL,
                applied_at timestamptz NOT NULL DEFAULT now(),
                PRIMARY KEY (module, version)
            );
            -- Databases created before migrations had modules: their rows were all core.
            DO $$ BEGIN
                IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                               WHERE table_schema = current_schema()
                                 AND table_name = 'schema_migrations' AND column_name = 'module')
                THEN
                    ALTER TABLE schema_migrations ADD COLUMN module text NOT NULL DEFAULT 'core';
                    ALTER TABLE schema_migrations ALTER COLUMN module DROP DEFAULT;
                    ALTER TABLE schema_migrations DROP CONSTRAINT schema_migrations_pkey;
                    ALTER TABLE schema_migrations ADD PRIMARY KEY (module, version);
                END IF;
            END $$;
            -- Checksums came later; rows from before get theirs on the next run.
            ALTER TABLE schema_migrations ADD COLUMN IF NOT EXISTS checksum text;",
    )
    .await?;
    for set in sets {
        let known: Vec<i32> = set.migrations.iter().map(|m| m.version).collect();
        let unknown = tx
            .query_typed_opt(
                "SELECT version FROM schema_migrations
                 WHERE module = $1 AND version <> ALL($2) ORDER BY version LIMIT 1",
                &[(&set.module, Type::TEXT), (&known, Type::INT4_ARRAY)],
            )
            .await?;
        if let Some(row) = unknown {
            return Err(MigrateError::Unknown {
                module: set.module.to_string(),
                version: row.try_get("version")?,
            });
        }
    }
    tx.commit().await?;

    for set in sets {
        for migration in set.migrations {
            let tx = client.transaction().await?;
            lock(&tx).await?;
            let applied = tx
                .query_typed_opt(
                    "SELECT name, checksum FROM schema_migrations WHERE module = $1 AND version = $2",
                    &[(&set.module, Type::TEXT), (&migration.version, Type::INT4)],
                )
                .await?;
            match applied {
                Some(row) => {
                    let applied_name: String = row.try_get("name")?;
                    if applied_name != migration.name {
                        return Err(MigrateError::Mismatch {
                            module: set.module.to_string(),
                            version: migration.version,
                            applied: applied_name,
                            code: migration.name.to_string(),
                        });
                    }
                    match row.try_get::<_, Option<String>>("checksum")? {
                        Some(applied) if applied != checksum(migration.sql) => {
                            return Err(MigrateError::Edited {
                                module: set.module.to_string(),
                                version: migration.version,
                            });
                        }
                        Some(_) => {}
                        // Applied before checksums existed: trust it and record one now.
                        None => {
                            tx.query_typed(
                                "UPDATE schema_migrations SET checksum = $3
                                 WHERE module = $1 AND version = $2",
                                &[
                                    (&set.module, Type::TEXT),
                                    (&migration.version, Type::INT4),
                                    (&checksum(migration.sql), Type::TEXT),
                                ],
                            )
                            .await?;
                        }
                    }
                }
                None => {
                    tx.batch_execute(migration.sql).await?;
                    tx.query_typed(
                        "INSERT INTO schema_migrations (module, version, name, checksum)
                         VALUES ($1, $2, $3, $4)",
                        &[
                            (&set.module, Type::TEXT),
                            (&migration.version, Type::INT4),
                            (&migration.name, Type::TEXT),
                            (&checksum(migration.sql), Type::TEXT),
                        ],
                    )
                    .await?;
                }
            }
            tx.commit().await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_ignores_carriage_returns_but_not_edits() {
        assert_eq!(checksum("A;\r\nB;\r\n"), checksum("A;\nB;\n"));
        assert_ne!(checksum("A;\nB;\n"), checksum("A;\nC;\n"));
        // FNV-1a of the empty string is its offset basis.
        assert_eq!(checksum(""), "fnv1a64:cbf29ce484222325");
    }

    #[test]
    fn errors_read_as_one_line() {
        let module = "core".to_string();
        for e in [
            MigrateError::Edited {
                module: module.clone(),
                version: 3,
            },
            MigrateError::Unknown { module, version: 4 },
        ] {
            let message = e.to_string();
            assert!(
                !message.contains('\n') && !message.contains("  "),
                "{message:?}"
            );
        }
    }

    const ONE: &[Migration] = &[Migration {
        version: 1,
        name: "a",
        sql: "",
    }];

    #[test]
    fn core_versions_increase() {
        assert!(check_sets(&[CORE]).is_ok());
    }

    #[test]
    fn same_version_in_different_modules_is_fine() {
        let sets = [
            MigrationSet {
                module: "core",
                migrations: ONE,
            },
            MigrationSet {
                module: "spore_x",
                migrations: ONE,
            },
        ];
        assert!(check_sets(&sets).is_ok());
    }

    #[test]
    fn rejects_duplicate_module() {
        let sets = [
            MigrationSet {
                module: "core",
                migrations: ONE,
            },
            MigrationSet {
                module: "core",
                migrations: ONE,
            },
        ];
        assert!(matches!(
            check_sets(&sets),
            Err(MigrateError::InvalidSet(_))
        ));
    }

    #[test]
    fn rejects_versions_out_of_order() {
        const TWO_THEN_ONE: &[Migration] = &[
            Migration {
                version: 2,
                name: "b",
                sql: "",
            },
            Migration {
                version: 1,
                name: "a",
                sql: "",
            },
        ];
        let sets = [MigrationSet {
            module: "core",
            migrations: TWO_THEN_ONE,
        }];
        assert!(matches!(
            check_sets(&sets),
            Err(MigrateError::InvalidSet(_))
        ));
    }
}
