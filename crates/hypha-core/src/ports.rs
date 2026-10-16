use thiserror::Error;
use uuid::Uuid;

use crate::Timestamp;

pub trait IdGenerator {
    fn new_uuid(&self) -> Uuid;
}

/// Where commands get "now" from: hosts read the system clock, tests use a fixed one
/// (`hypha_testkit::FixedClock`), so a command's timestamps are exact in assertions.
pub trait Clock {
    fn now(&self) -> Timestamp;
}

/// How much a job step did, and whether it stopped at its limit with more to do
/// (design note 0016, EV10): a runner repeats the step while there's more and waits its
/// interval when there isn't. Every crate's jobs (dispatching, pruning, search sync)
/// answer with it, so a Workers Cron trigger can drive them all the same way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub done: u32,
    pub more: bool,
}

impl Progress {
    /// `done` items of a batch of at most `limit`: a full batch may have more behind it.
    pub fn of(done: u32, limit: u32) -> Self {
        Self {
            done,
            more: done >= limit,
        }
    }
}

/// Randomness for secrets: tokens, salts, nonces, recovery codes (design note 0008).
/// Hosts use the operating system's generator; tests use a deterministic one. Never a
/// seeded or predictable generator in production: these bytes are what keep a session
/// token unguessable.
pub trait Random {
    fn fill(&self, bytes: &mut [u8]);
}

/// Sends a domain's messages, e.g. `hypha_users::StaffEmail` (design note 0008, A10).
/// Domain crates say *what* to tell whom; the host renders and delivers it (email
/// templates, links to its admin panel), so domain code never sees URLs.
///
/// Commands send after their write commits. The outbox (0002) will later stage
/// messages in the same batch instead.
#[allow(async_fn_in_trait)]
pub trait Notifier<M> {
    async fn notify(&self, message: M) -> Result<(), NotifyError>;
}

/// A message could not be handed to the delivery service. The text is for logs.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
#[error("could not send the message: {0}")]
pub struct NotifyError(pub String);

impl crate::ErrorCode for NotifyError {
    fn code(&self) -> &'static str {
        "notification_failed"
    }
}

/// Writes to several aggregates that commit together or not at all (design note 0007,
/// O1): one transaction on Postgres, one locked step in memory.
///
/// Domain code loads, decides, then stages its writes into a batch through each crate's
/// `Stage*` trait (`hypha_cart::StageCarts`, …), which the adapter's `Batch` implements;
/// `commit` is the only point that opens a transaction. A command that writes several
/// aggregates goes through here; one aggregate still uses its repo's `insert`/`update`.
#[allow(async_fn_in_trait)]
pub trait HasBatch {
    /// The adapter's list of staged writes.
    type Batch;
    fn batch(&self) -> Self::Batch;
    /// Writes everything staged, or nothing. Fails like the single writes would:
    /// `StaleVersion` if any aggregate changed since it was loaded, `Duplicate(key)` on a
    /// unique key.
    async fn commit(&self, batch: Self::Batch) -> Result<(), RepoError>;
}

/// Borrowed ports are ports too, so a host can lend them out per request.
impl<T: HasBatch + ?Sized> HasBatch for &T {
    type Batch = T::Batch;
    fn batch(&self) -> Self::Batch {
        (**self).batch()
    }
    async fn commit(&self, batch: Self::Batch) -> Result<(), RepoError> {
        (**self).commit(batch).await
    }
}

/// What a repository can report. Adapters (in-memory, Postgres) must map their
/// failures onto these, so commands behave the same on every backend.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum RepoError {
    /// The backend failed (connection, corrupt data, ...). Never shown to clients.
    #[error("storage backend error: {0}")]
    Backend(String),
    /// An update was based on an old version: someone else changed the record since it
    /// was loaded (optimistic concurrency, see design note 0001). Safe to retry.
    #[error("the record was changed by someone else since it was loaded")]
    StaleVersion,
    /// A write would break a uniqueness rule. The key names the rule (e.g.
    /// `"product_handle"`); domain crates own the constants and map them to precise
    /// errors like `HandleTaken`.
    #[error("a record with the same {0} already exists")]
    Duplicate(&'static str),
    /// A write would break a reference between records: it points at one that doesn't
    /// exist, or removes one that others point at. The key names the rule (e.g.
    /// `"asset_usage_asset"`); commands map it to precise errors like `AssetInUse`.
    #[error("the write breaks the {0} reference")]
    Reference(&'static str),
}

impl crate::ErrorCode for RepoError {
    fn code(&self) -> &'static str {
        match self {
            RepoError::Backend(_) => "storage_error",
            RepoError::StaleVersion => "concurrent_modification",
            // Commands map the keys they know to specific codes; this is the fallback.
            RepoError::Duplicate(_) | RepoError::Reference(_) => "conflict",
        }
    }
}

/// Finishes a write guarded by a pre-check (a unique key looked up first, for a precise
/// error). If the write still fails on `key`, another request took it between the check
/// and the write, so `conflict` (the same lookup as the pre-check) runs again and the
/// race reports like the normal case. Other results pass through.
///
/// `conflict` is a future, and futures do nothing until awaited, so the lookup only runs
/// on that failure. Passing a future rather than an async closure keeps the command's
/// own future `Send` where a host needs it.
pub async fn report_duplicate<E: From<RepoError>>(
    write: Result<(), RepoError>,
    key: &'static str,
    conflict: impl Future<Output = Result<Option<E>, E>>,
) -> Result<(), E> {
    match write {
        Err(RepoError::Duplicate(broken)) if broken == key => {
            Err(conflict.await?.unwrap_or(RepoError::Duplicate(key).into()))
        }
        other => Ok(other?),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq)]
    enum Error {
        Taken(u32),
        Repo(RepoError),
    }

    impl From<RepoError> for Error {
        fn from(e: RepoError) -> Self {
            Error::Repo(e)
        }
    }

    fn block_on<F: Future>(f: F) -> F::Output {
        // The futures here never wait on anything, so one poll finishes them.
        let mut f = std::pin::pin!(f);
        let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
        match f.as_mut().poll(&mut cx) {
            std::task::Poll::Ready(v) => v,
            std::task::Poll::Pending => unreachable!("test futures are always ready"),
        }
    }

    #[test]
    fn a_lost_race_reports_the_owner_the_lookup_finds() {
        let result = block_on(report_duplicate(
            Err(RepoError::Duplicate("handle")),
            "handle",
            async { Ok(Some(Error::Taken(7))) },
        ));
        assert_eq!(result, Err(Error::Taken(7)));
    }

    #[test]
    fn an_owner_gone_again_falls_back_to_the_duplicate() {
        let result = block_on(report_duplicate(
            Err(RepoError::Duplicate("handle")),
            "handle",
            async { Ok::<_, Error>(None) },
        ));
        assert_eq!(result, Err(Error::Repo(RepoError::Duplicate("handle"))));
    }

    #[test]
    fn other_results_pass_through_without_a_lookup() {
        let looked_up = std::cell::Cell::new(false);
        let lookup = async {
            looked_up.set(true);
            Ok::<_, Error>(None)
        };
        assert_eq!(block_on(report_duplicate(Ok(()), "handle", lookup)), Ok(()));
        assert!(!looked_up.get());
        assert_eq!(
            block_on(report_duplicate(
                Err(RepoError::Duplicate("other")),
                "handle",
                async { Ok::<_, Error>(None) },
            )),
            Err(Error::Repo(RepoError::Duplicate("other")))
        );
    }
}
