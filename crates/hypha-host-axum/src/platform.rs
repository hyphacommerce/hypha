//! The native host's capabilities (`hypha_http::host`): the system clock, UUIDv7 IDs,
//! the operating system's randomness, Tokio's blocking threads for CPU-heavy work, and
//! a few slots for work after the response.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use hypha_core::{Clock, IdGenerator, Timestamp};
use hypha_http::host::{HostBackground, HostClock, HostHttp, HostIds, HostRandom, Offload};
use hypha_http::state::Runtime;
use uuid::Uuid;

/// Real IDs: UUIDv7 (time-ordered), generated from the system clock and RNG.
#[derive(Default)]
pub struct UuidV7Ids;

impl IdGenerator for UuidV7Ids {
    fn new_uuid(&self) -> Uuid {
        Uuid::now_v7()
    }
}

/// The real clock. Only hosts read it; commands get it as a `Clock`.
#[derive(Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        // A clock set before 1970 reads as 1970; i64 microseconds last until year 294247.
        let micros = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| {
                i64::try_from(since.as_micros()).unwrap_or(i64::MAX)
            });
        Timestamp::from_unix_micros(micros)
    }
}

/// The native runtime: the system clock, UUIDv7, the OS's randomness, and Tokio's
/// blocking threads for password hashing.
pub fn runtime() -> Runtime {
    Runtime {
        clock: HostClock::new(SystemClock),
        ids: HostIds::new(UuidV7Ids),
        random: HostRandom::new(|bytes| {
            // Without the OS generator no token or key can be made safely, and there is
            // no sensible way to continue: stop, like the standard library does when it
            // can't seed its hash maps.
            if let Err(e) = getrandom::fill(bytes) {
                panic!("the operating system's random number generator failed: {e}");
            }
        }),
        offload: Offload::new(|job| {
            Box::pin(async move {
                tokio::task::spawn_blocking(job)
                    .await
                    .map_err(|e| e.to_string())
            })
        }),
    }
}

/// The media storage's HTTP: `reqwest`, through the S3 adapter's client.
pub fn host_http() -> Result<HostHttp, String> {
    let client = hypha_store_s3::ReqwestClient::new()?;
    Ok(HostHttp::new(move |request| {
        let client = client.clone();
        Box::pin(async move { hypha_store_s3::HttpClient::send(&client, request).await })
    }))
}

/// Work that runs after its response went out (password reset requests, buyers'
/// emails), with a fixed number of slots. When they are full, new work is dropped and
/// logged: anonymous requests can't pile up tasks that each wait for a pooled
/// connection, and the client gets no signal it could probe with. Shutdown waits for the
/// work in flight (`drain`).
#[derive(Clone)]
pub struct Background {
    slots: Arc<tokio::sync::Semaphore>,
}

impl Background {
    /// Few on purpose: each slot may hold a pooled connection.
    pub const SLOTS: u32 = 4;

    /// Spawns `work` if a slot is free; `false` (and nothing runs) if not.
    pub fn spawn(&self, work: impl Future<Output = ()> + Send + 'static) -> bool {
        let Ok(slot) = Arc::clone(&self.slots).try_acquire_owned() else {
            return false;
        };
        tokio::spawn(async move {
            work.await;
            drop(slot);
        });
        true
    }

    /// The same slots, as `AppState`'s `HostBackground`.
    pub fn host(&self) -> HostBackground {
        let this = self.clone();
        HostBackground::new(move |work| this.spawn(work))
    }

    /// Waits up to `limit` for the work in flight to finish; `false` if it didn't. Takes
    /// every slot, so nothing new starts meanwhile.
    pub async fn drain(&self, limit: std::time::Duration) -> bool {
        tokio::time::timeout(limit, self.slots.acquire_many(Self::SLOTS))
            .await
            .is_ok_and(|permits| permits.is_ok())
    }
}

impl Default for Background {
    fn default() -> Self {
        Self {
            slots: Arc::new(tokio::sync::Semaphore::new(Self::SLOTS as usize)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn background_work_beyond_the_slots_is_dropped_and_drain_waits() {
        let background = Background::default();
        let (release, wait) = tokio::sync::watch::channel(false);
        for _ in 0..Background::SLOTS {
            let mut wait = wait.clone();
            assert!(background.spawn(async move {
                let _ = wait.wait_for(|go| *go).await;
            }));
        }
        assert!(!background.spawn(async {}));
        assert!(!background.drain(std::time::Duration::from_millis(20)).await);
        release.send(true).unwrap();
        assert!(background.drain(std::time::Duration::from_secs(5)).await);
    }
}
