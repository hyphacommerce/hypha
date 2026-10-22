//! Databases for tests: each test gets a database of its own, copied from a template that
//! is migrated once per set of migrations. Copying a template is a file copy on the
//! server; running every migration again for each test cost about a second.
//!
//! The server is `HYPHA_TEST_DATABASE_URL` (the compose `postgres-test`, tuned for
//! throwaway data), else `DATABASE_URL`. Native only: it opens its own connections.

use std::time::Duration;

use tokio_postgres::{Client, Config, NoTls};

use crate::{MigrationSet, migrate};

/// Serializes building a template across test processes (nextest runs one per test).
const TEMPLATE_LOCK: i64 = 0x6879_7068_6174_6d70;

/// The test server's URL: `HYPHA_TEST_DATABASE_URL`, else `DATABASE_URL`.
pub fn test_database_url() -> Option<String> {
    std::env::var("HYPHA_TEST_DATABASE_URL")
        .or_else(|_| std::env::var("DATABASE_URL"))
        .ok()
}

/// A test's own database, dropped when this is (pass it to [`TestDatabase::keep`] to
/// leave it; the next run with the same name drops it first).
pub struct TestDatabase {
    url: String,
    server: String,
    name: String,
    dropped: bool,
}

impl TestDatabase {
    /// A fresh copy of the database `sets` migrate, named `name` (at most 63 bytes,
    /// lowercase letters, digits and `_`), on the test server. `None` without a server URL.
    pub async fn new(name: &str, sets: &[MigrationSet]) -> Option<TestDatabase> {
        let server = test_database_url()?;
        assert!(
            name.len() <= 63
                && name
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'),
            "test database name {name:?}: at most 63 of a-z, 0-9 and _"
        );
        let admin = connect(&server).await;
        let template = template(&admin, &server, sets).await;
        // One statement per call: Postgres runs a multi-statement batch as one
        // transaction, and creating or dropping a database refuses that.
        for statement in [
            format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"),
            format!("CREATE DATABASE {name} TEMPLATE {template}"),
        ] {
            admin
                .batch_execute(&statement)
                .await
                .unwrap_or_else(|e| panic!("create test database {name}: {e}"));
        }
        Some(TestDatabase {
            url: with_database(&server, name),
            server,
            name: name.to_string(),
            dropped: false,
        })
    }

    /// The URL that connects to this database.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Leaves the database in place, for a test that hands its connection on.
    pub fn keep(mut self) -> String {
        self.dropped = true;
        std::mem::take(&mut self.url)
    }
}

impl Drop for TestDatabase {
    fn drop(&mut self) {
        if self.dropped {
            return;
        }
        // Drop can't await, and the test's runtime may be the one dropping this: a thread
        // with a runtime of its own does it.
        let (server, name) = (self.server.clone(), self.name.clone());
        let done = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("a runtime to drop the test database");
            runtime.block_on(async {
                let admin = connect(&server).await;
                let _ = admin
                    .batch_execute(&format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"))
                    .await;
            });
        });
        let _ = done.join();
    }
}

/// The template for `sets`, built under a lock if it isn't there yet. Its name carries a
/// fingerprint of every migration, so a changed or added migration gets a new template.
async fn template(admin: &Client, server: &str, sets: &[MigrationSet]) -> String {
    let name = format!("hypha_template_{:016x}", fingerprint(sets));
    if exists(admin, &name).await {
        return name;
    }
    admin
        .execute("SELECT pg_advisory_lock($1)", &[&TEMPLATE_LOCK])
        .await
        .expect("lock the template");
    if !exists(admin, &name).await {
        // Built under another name and renamed when complete, so a test killed while
        // building never leaves a half-migrated template behind.
        let building = format!("{name}_building");
        for statement in [
            format!("DROP DATABASE IF EXISTS {building} WITH (FORCE)"),
            format!("CREATE DATABASE {building}"),
        ] {
            admin
                .batch_execute(&statement)
                .await
                .expect("create the template");
        }
        {
            let (mut client, connection) = config(&with_database(server, &building))
                .connect(NoTls)
                .await
                .expect("connect to the template");
            let connection = tokio::spawn(connection);
            migrate(&mut client, sets)
                .await
                .expect("migrate the template");
            drop(client);
            let _ = connection.await;
        }
        // The server may not have seen the connection go yet.
        let mut tries = 0;
        loop {
            let renamed = admin
                .batch_execute(&format!("ALTER DATABASE {building} RENAME TO {name}"))
                .await;
            match renamed {
                Ok(()) => break,
                Err(_) if tries < 50 => {
                    tries += 1;
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                Err(e) => panic!("finish the template: {e}"),
            }
        }
        admin
            .batch_execute(&format!(
                "ALTER DATABASE {name} WITH IS_TEMPLATE true ALLOW_CONNECTIONS false"
            ))
            .await
            .expect("mark the template");
    }
    admin
        .execute("SELECT pg_advisory_unlock($1)", &[&TEMPLATE_LOCK])
        .await
        .expect("unlock the template");
    name
}

async fn exists(admin: &Client, name: &str) -> bool {
    admin
        .query_opt("SELECT 1 FROM pg_database WHERE datname = $1", &[&name])
        .await
        .expect("look for the template")
        .is_some()
}

/// FNV-1a over every set's module and every migration's version, name and SQL.
fn fingerprint(sets: &[MigrationSet]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |bytes: &[u8]| {
        for byte in bytes.iter().chain([&0xff]) {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
    };
    for set in sets {
        feed(set.module.as_bytes());
        for m in set.migrations {
            feed(&m.version.to_le_bytes());
            feed(m.name.as_bytes());
            feed(m.sql.as_bytes());
        }
    }
    hash
}

fn config(url: &str) -> Config {
    crate::database_config(url).unwrap_or_else(|e| panic!("test database URL {url}: {e}"))
}

async fn connect(url: &str) -> Client {
    let (client, connection) = config(url)
        .connect(NoTls)
        .await
        .unwrap_or_else(|e| panic!("tests need Postgres at {url} (`docker compose up -d`): {e}"));
    tokio::spawn(connection);
    client
}

/// `url` with its database replaced by `database`, the query kept.
fn with_database(url: &str, database: &str) -> String {
    let (base, query) = match url.split_once('?') {
        Some((base, query)) => (base, Some(query)),
        None => (url, None),
    };
    let authority_start = base.find("://").map_or(0, |i| i + 3);
    let host_end = base[authority_start..]
        .find('/')
        .map_or(base.len(), |i| authority_start + i);
    let mut out = format!("{}/{database}", &base[..host_end]);
    if let Some(query) = query {
        out.push('?');
        out.push_str(query);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_database_is_replaced_and_the_query_kept() {
        assert_eq!(
            with_database("postgres://u:p@h:5433/hypha?sslmode=disable", "t_1"),
            "postgres://u:p@h:5433/t_1?sslmode=disable"
        );
        assert_eq!(
            with_database("postgres://u:p@h:5433", "t_1"),
            "postgres://u:p@h:5433/t_1"
        );
        assert_eq!(with_database("postgres://h/x", "t_1"), "postgres://h/t_1");
    }

    #[test]
    fn a_changed_migration_changes_the_fingerprint() {
        const A: &[crate::Migration] = &[crate::Migration {
            version: 1,
            name: "a",
            sql: "x",
        }];
        const B: &[crate::Migration] = &[crate::Migration {
            version: 1,
            name: "a",
            sql: "y",
        }];
        let set = |migrations| MigrationSet {
            module: "core",
            migrations,
        };
        assert_ne!(fingerprint(&[set(A)]), fingerprint(&[set(B)]));
        assert_eq!(fingerprint(&[set(A)]), fingerprint(&[set(A)]));
    }
}
