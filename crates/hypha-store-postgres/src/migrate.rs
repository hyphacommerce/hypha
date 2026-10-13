use std::{collections::HashSet, fmt};

use tokio_postgres::{Client, types::Type};

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
        }
    }
}

impl std::error::Error for MigrateError {}

impl From<tokio_postgres::Error> for MigrateError {
    fn from(e: tokio_postgres::Error) -> Self {
        MigrateError::Db(e)
    }
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

/// Applies every migration not applied yet, module by module in the given order (core
/// first, then spores), each in its own transaction.
///
/// Run it from native code (a CLI, host startup), never from a Worker. Safe to run
/// concurrently: the `schema_migrations` lock makes other runners wait.
pub async fn migrate(client: &mut Client, sets: &[MigrationSet]) -> Result<(), MigrateError> {
    check_sets(sets)?;
    client
        .batch_execute(
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
            END $$;",
        )
        .await?;

    for set in sets {
        for migration in set.migrations {
            let tx = client.transaction().await?;
            tx.batch_execute("LOCK TABLE schema_migrations IN EXCLUSIVE MODE")
                .await?;
            let applied = tx
                .query_typed_opt(
                    "SELECT name FROM schema_migrations WHERE module = $1 AND version = $2",
                    &[(&set.module, Type::TEXT), (&migration.version, Type::INT4)],
                )
                .await?;
            match applied {
                Some(row) => {
                    let applied_name: String = row.get(0);
                    if applied_name != migration.name {
                        return Err(MigrateError::Mismatch {
                            module: set.module.to_string(),
                            version: migration.version,
                            applied: applied_name,
                            code: migration.name.to_string(),
                        });
                    }
                }
                None => {
                    tx.batch_execute(migration.sql).await?;
                    tx.query_typed(
                        "INSERT INTO schema_migrations (module, version, name) VALUES ($1, $2, $3)",
                        &[
                            (&set.module, Type::TEXT),
                            (&migration.version, Type::INT4),
                            (&migration.name, Type::TEXT),
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
