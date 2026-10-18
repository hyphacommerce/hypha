//! The job runner (design note 0016, section 7): one task per job, each running its
//! step (`hypha_http::jobs`) every interval, again at once while there's more, and
//! stopping after the step in flight (`Jobs::stop`).
//!
//! An idle job looks less and less often (design note 0025, §6): each round that finds
//! nothing doubles the wait, up to `jobs.idle_interval`, and finding work brings it back
//! to the interval. Work doesn't wait for that: a write that commits events (or retries
//! a delivery) wakes the dispatcher (`AfterWrites`, through `Wake`), a dispatch step that did something wakes search
//! sync, and queueing an email (the `Mailer` handler, a resend) wakes the send job
//! (`MailQueued`). So a quiet store costs the database a query a minute, not several a
//! second.

use std::{future::Future, sync::Arc, time::Duration};

use hypha_http::host::{AfterWrites, MailQueued};
use hypha_http::jobs::{self, Ran, Step};
use hypha_http::state::SharedState;
use tokio::{
    sync::{Notify, watch},
    task::JoinHandle,
};

use crate::limiter::MemoryRateLimiter;
use crate::storage::PgStorage;

/// The running jobs. Dropping it without `stop` leaves them running until the process
/// exits.
pub struct Jobs {
    stop: watch::Sender<bool>,
    tasks: Vec<JoinHandle<()>>,
}

impl Jobs {
    /// Tells every job to stop after the step it's in, and waits up to `limit` for them;
    /// `false` if some didn't finish in time.
    pub async fn stop(self, limit: Duration) -> bool {
        let _ = self.stop.send(true);
        let all = async {
            for task in self.tasks {
                // A job that panicked is done too.
                let _ = task.await;
            }
        };
        tokio::time::timeout(limit, all).await.is_ok()
    }
}

/// What wakes the jobs before their timers.
#[derive(Default, Clone)]
pub struct Wake {
    pub dispatch: Arc<Notify>,
    pub search_sync: Arc<Notify>,
    pub send_notifications: Arc<Notify>,
}

impl Wake {
    /// The host's `AfterWrites`: a write that gave it work wakes the dispatcher. A wake while it works
    /// isn't lost: `Notify` keeps it for its next wait.
    pub fn after_writes(&self) -> AfterWrites {
        let dispatch = Arc::clone(&self.dispatch);
        AfterWrites::new(move || dispatch.notify_one())
    }

    /// The mail host's `MailQueued`: queueing an email wakes the send job.
    pub fn mail_queued(&self) -> MailQueued {
        let send = Arc::clone(&self.send_notifications);
        MailQueued::new(move || send.notify_one())
    }
}

/// Starts the jobs for a Postgres host.
pub fn start(state: SharedState<PgStorage>, wake: Wake, limiter: Arc<MemoryRateLimiter>) -> Jobs {
    let (stop, stopped) = watch::channel(false);
    let mut tasks = Vec::new();

    let jobs = state.config.jobs;
    let idle = jobs.idle_interval;

    // Delivering events: fan out new ones and run what's due (every 2 seconds by default,
    // less often while there's nothing, and at once after a write).
    if jobs.dispatch.enabled {
        let s = Arc::clone(&state);
        let search = Arc::clone(&wake.search_sync);
        tasks.push(every(
            &stopped,
            Timer {
                name: "dispatch",
                interval: jobs.dispatch.interval,
                idle,
                wake: Some(Arc::clone(&wake.dispatch)),
            },
            move || {
                let s = Arc::clone(&s);
                let search = Arc::clone(&search);
                async move {
                    let ran = jobs::dispatch(&s).await;
                    // Its handlers may have marked products for search.
                    if matches!(ran, Ok(r) if !r.idle) {
                        search.notify_one();
                    }
                    ran
                }
            },
        ));
    }

    // Old events, idempotency keys and ended sessions (hourly by default).
    if jobs.prune.enabled {
        let s = Arc::clone(&state);
        tasks.push(every(
            &stopped,
            Timer::fixed("prune", jobs.prune.interval),
            move || {
                let s = Arc::clone(&s);
                async move { jobs::prune(&s).await }
            },
        ));
    }

    // Rebuilding the search documents events marked (every 2 seconds by default, less
    // often while there's nothing, and at once after the dispatcher ran handlers).
    if jobs.search_sync.enabled {
        let s = Arc::clone(&state);
        tasks.push(every(
            &stopped,
            Timer {
                name: "search_sync",
                interval: jobs.search_sync.interval,
                idle,
                wake: Some(Arc::clone(&wake.search_sync)),
            },
            move || {
                let s = Arc::clone(&s);
                async move { jobs::search_sync(&s).await }
            },
        ));
    }

    // Sending queued emails (every 2 seconds by default, less often while there's
    // nothing, and at once when the mailer or a resend queued one).
    if jobs.send_notifications.enabled {
        let s = Arc::clone(&state);
        tasks.push(every(
            &stopped,
            Timer {
                name: "send_notifications",
                interval: jobs.send_notifications.interval,
                idle,
                wake: Some(Arc::clone(&wake.send_notifications)),
            },
            move || {
                let s = Arc::clone(&s);
                async move { jobs::send_notifications(&s).await }
            },
        ));
    }

    if jobs.media_sweep.enabled && state.media.is_some() {
        let s = Arc::clone(&state);
        tasks.push(every(
            &stopped,
            Timer::fixed("media_sweep", jobs.media_sweep.interval),
            move || {
                let s = Arc::clone(&s);
                async move { jobs::media_sweep(&s).await }
            },
        ));
    }

    // Idle rate-limit keys, so rotating addresses can't grow memory. Housekeeping, not
    // a job: it runs even with `jobs.enabled = false`.
    tasks.push(every(
        &stopped,
        Timer::fixed("rate_limits", Duration::from_secs(60)),
        move || {
            let limiter = Arc::clone(&limiter);
            async move {
                limiter.forget_idle();
                Ok(Ran {
                    more: false,
                    idle: false,
                    retry_at: None,
                })
            }
        },
    ));

    Jobs { stop, tasks }
}

/// When a job runs: every `interval`, stretching to `idle` while it finds nothing, and
/// at once when `wake` is notified.
struct Timer {
    name: &'static str,
    interval: Duration,
    idle: Duration,
    wake: Option<Arc<Notify>>,
}

impl Timer {
    /// Every `interval`, idle or not (the hourly jobs, housekeeping).
    fn fixed(name: &'static str, interval: Duration) -> Self {
        Self {
            name,
            interval,
            idle: interval,
            wake: None,
        }
    }

    /// The wait after a round: back to the interval after work, else twice the last,
    /// at most `idle` (and never under the interval).
    fn next_wait(&self, last: Duration, worked: bool) -> Duration {
        if worked {
            self.interval
        } else {
            last.saturating_mul(2).min(self.idle).max(self.interval)
        }
    }
}

/// Runs `step` on `timer` (repeating it at once while it reports more) until told to
/// stop. A failing step is logged and tried again next round.
fn every<F, Fut>(stopped: &watch::Receiver<bool>, timer: Timer, step: F) -> JoinHandle<()>
where
    F: Fn() -> Fut + Send + 'static,
    Fut: Future<Output = Step> + Send,
{
    let mut stopped = stopped.clone();
    tokio::spawn(async move {
        let mut wait = timer.interval;
        loop {
            let woken = async {
                match &timer.wake {
                    Some(wake) => wake.notified().await,
                    None => std::future::pending().await,
                }
            };
            tokio::select! {
                _ = stopped.changed() => return,
                () = tokio::time::sleep(wait) => {}
                () = woken => {}
            }
            let mut worked = false;
            loop {
                match step().await {
                    Ok(ran) => {
                        worked |= !ran.idle;
                        if ran.more && !*stopped.borrow() {
                            continue;
                        }
                        break;
                    }
                    Err(e) => {
                        tracing::warn!(job = timer.name, "jobs: step failed: {e}");
                        break;
                    }
                }
            }
            wait = timer.next_wait(wait, worked);
            if *stopped.borrow() {
                return;
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timer() -> Timer {
        Timer {
            name: "test",
            interval: Duration::from_secs(2),
            idle: Duration::from_secs(60),
            wake: None,
        }
    }

    #[test]
    fn an_idle_job_waits_twice_as_long_each_round() {
        assert_eq!(
            timer().next_wait(Duration::from_secs(2), false),
            Duration::from_secs(4)
        );
    }

    #[test]
    fn an_idle_jobs_wait_stops_at_the_idle_interval() {
        assert_eq!(
            timer().next_wait(Duration::from_secs(32), false),
            Duration::from_secs(60)
        );
    }

    #[test]
    fn finding_work_brings_the_wait_back_to_the_interval() {
        assert_eq!(
            timer().next_wait(Duration::from_secs(60), true),
            Duration::from_secs(2)
        );
    }

    #[test]
    fn a_fixed_job_keeps_its_interval_when_idle() {
        let hourly = Timer::fixed("prune", Duration::from_secs(3600));
        assert_eq!(
            hourly.next_wait(Duration::from_secs(3600), false),
            Duration::from_secs(3600)
        );
    }
}
