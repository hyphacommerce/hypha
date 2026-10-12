use futures_util::lock::{Mutex, MutexGuard};
use tokio_postgres::Client;

/// Something that holds a connected `tokio_postgres::Client`: the client itself, a
/// borrowed one, or (in a host) a connection checked out of a pool.
pub trait PgConnection {
    fn client(&mut self) -> &mut Client;
}

impl PgConnection for Client {
    fn client(&mut self) -> &mut Client {
        self
    }
}

impl PgConnection for &mut Client {
    fn client(&mut self) -> &mut Client {
        self
    }
}

/// Postgres implementation of every repository port.
///
/// The connection sits behind an async mutex, so one operation uses it at a time: a
/// transaction can't interleave with another query on the same connection, even if the
/// store is shared. Transactions need `&mut Client`, which the mutex hands out.
pub struct PgStore<C> {
    conn: Mutex<C>,
}

impl<C: PgConnection> PgStore<C> {
    pub fn new(conn: C) -> Self {
        Self {
            conn: Mutex::new(conn),
        }
    }

    pub(crate) async fn conn(&self) -> MutexGuard<'_, C> {
        self.conn.lock().await
    }
}
