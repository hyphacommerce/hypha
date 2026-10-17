//! Background jobs (design note 0016, section 7): one runner for everything that runs on
//! a timer rather than for a request. Each job is a step that does a bounded amount of
//! work and says whether there's more; the runner repeats it while there is, then waits
//! its interval (`[jobs]`, design note 0017). Stopping waits for the steps in flight to finish (`Jobs::stop`).
//!
//! An idle job looks less and less often (design note 0025, §6): each step that finds
//! nothing doubles the wait, up to `jobs.idle_interval`, and finding work brings it back
//! to the interval. Work doesn't wait for that: a successful write request wakes the
//! dispatcher (`wake_after_writes`), and a dispatch step that did something wakes
//! search sync, whose marks the handlers it ran may have left. So a quiet store costs
//! the database a query a minute, not several a second.
//!
//! The steps are the domain crates' plain functions (`hypha_dispatch::fan_out`, …), so
//! a Workers host calls the same ones from a Cron trigger instead of this runner.

use std::{future::Future, sync::Arc, time::Duration};

use axum::{
    extract::{Request, State},
    http::Method,
    middleware::Next,
    response::Response,
};

use hypha_core::{Outcome, Progress};
use hypha_dispatch::{
    Prepared, fan_out, handle, has_time, lease_due, prepare, prune_events, prune_idempotency_keys,
    prune_sessions, record, send,
};
use tokio::{
    sync::{Notify, watch},
    task::JoinHandle,
};

use crate::{
    handlers::HANDLERS,
    state::{AppState, PgStorage, SharedState},
};

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

/// What a step did: whether there's more to do right away, and whether it found
/// anything at all (an idle step makes the job wait longer).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Ran {
    pub(crate) more: bool,
    pub(crate) idle: bool,
}

impl Ran {
    fn of(progress: Progress) -> Self {
        Self {
            more: progress.more,
            idle: progress.done == 0,
        }
    }
}

type Step = Result<Ran, String>;

/// What wakes the jobs before their timers.
#[derive(Default)]
pub struct Wake {
    pub dispatch: Arc<Notify>,
    pub search_sync: Arc<Notify>,
}

/// A layer: after a successful request that may have written (anything but GET, HEAD
/// and OPTIONS), wakes the dispatcher to deliver the events it recorded. A wake while
/// the dispatcher works isn't lost: `Notify` keeps it for its next wait.
pub async fn wake_after_writes(
    State(dispatch): State<Arc<Notify>>,
    request: Request,
    next: Next,
) -> Response {
    let writes = !matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    );
    let response = next.run(request).await;
    if writes && response.status().is_success() {
        dispatch.notify_one();
    }
    response
}

/// Starts the jobs for a Postgres host.
pub fn start(state: SharedState<PgStorage>) -> Jobs {
    let (stop, stopped) = watch::channel(false);
    let mut tasks = Vec::new();

    let jobs = state.config.jobs;
    let idle = jobs.idle_interval;

    // Delivering events: fan out new ones and run what's due (every 2 seconds by default,
    // less often while there's nothing, and at once after a write).
    if jobs.dispatch.enabled {
        let s = Arc::clone(&state);
        tasks.push(every(
            &stopped,
            Timer {
                name: "dispatch",
                interval: jobs.dispatch.interval,
                idle,
                wake: Some(Arc::clone(&state.wake.dispatch)),
            },
            move || {
                let s = Arc::clone(&s);
                async move { dispatch(&s).await }
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
                async move { prune(&s).await }
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
                wake: Some(Arc::clone(&state.wake.search_sync)),
            },
            move || {
                let s = Arc::clone(&s);
                async move { search_sync(&s).await }
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
                async move { media_sweep(&s).await }
            },
        ));
    }

    // Idle rate-limit keys, so rotating addresses can't grow memory. Housekeeping, not
    // a job: it runs even with `jobs.enabled = false`.
    let s = Arc::clone(&state);
    tasks.push(every(
        &stopped,
        Timer::fixed("rate_limits", Duration::from_secs(60)),
        move || {
            let s = Arc::clone(&s);
            async move {
                s.limiter.forget_idle();
                Ok(Ran {
                    more: false,
                    idle: false,
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

/// Fans out new events, then runs the due deliveries. The lease and the outcomes each
/// take a connection briefly; each delivery gets ports to load its app or run its
/// handler, and lets them go before a webhook is sent, so no connection waits on another
/// server (design note 0016, EV9). Deliveries the lease can't cover any more are left
/// for the next lease (`has_time`).
pub(crate) async fn dispatch(state: &AppState<PgStorage>) -> Step {
    let no_connection = |_| "no connection".to_string();
    let (fanned, leased) = {
        let ports = state.public_ports().await.map_err(no_connection)?;
        let fanned = fan_out(&ports, &state.clock, &state.config.dispatch, &HANDLERS)
            .await
            .map_err(|e| e.to_string())?;
        let leased = lease_due(&ports, &state.clock, &state.config.dispatch)
            .await
            .map_err(|e| e.to_string())?;
        (fanned, leased)
    };
    let mut outcomes = Vec::with_capacity(leased.len());
    for delivery in leased.iter().take_while(|d| has_time(d, &state.clock)) {
        let prepared = {
            let ports = state.public_ports().await.map_err(no_connection)?;
            match prepare(&ports, &state.clock, &state.config.dispatch, delivery).await {
                Prepared::Handler => Prepared::Done(
                    handle(
                        &ports,
                        &state.clock,
                        &state.config.dispatch,
                        &HANDLERS,
                        delivery,
                    )
                    .await,
                ),
                other => other,
            }
        };
        let outcome = match prepared {
            Prepared::Webhook(app) => {
                send(
                    &state.webhooks(),
                    &state.clock,
                    &state.config.dispatch,
                    &app,
                    delivery,
                )
                .await
            }
            Prepared::Done(outcome) => outcome,
            // Handled above, with the ports.
            Prepared::Handler => continue,
        };
        if let hypha_core::DeliveryOutcome::Failed { error } = &outcome {
            tracing::warn!(
                destination = delivery.destination,
                event = %delivery.event.id,
                "jobs: delivery failed for good: {error}"
            );
        }
        outcomes.push(Outcome::of(delivery, outcome));
    }
    let ports = state.public_ports().await.map_err(no_connection)?;
    record(&ports, &outcomes).await.map_err(|e| e.to_string())?;
    let idle = fanned.done == 0 && leased.is_empty();
    if !idle {
        // The handlers it ran may have marked products for search.
        state.wake.search_sync.notify_one();
    }
    Ok(Ran {
        more: fanned.more || leased.len() >= state.config.dispatch.batch as usize,
        idle,
    })
}

/// Rebuilds a batch of the search documents events marked (design note 0019, section
/// 5). One connection for the step: the built-in engine is the database.
pub(crate) async fn search_sync(state: &AppState<PgStorage>) -> Step {
    use crate::state::Storage;
    let ports = state
        .public_ports()
        .await
        .map_err(|_| "no connection".to_string())?;
    let synced = hypha_search::sync(
        &ports,
        &state.storage.search(&ports),
        &state.clock,
        &state.config.search_sync,
    )
    .await
    .map_err(|e| e.to_string())?;
    for (product, error) in &synced.failed {
        tracing::warn!(%product, "search: rebuilding a product failed, retried later: {error}");
    }
    Ok(Ran::of(synced.progress))
}

/// Deletes uploads abandoned for over `media.abandoned_after` (design note 0013,
/// section 3).
async fn media_sweep(state: &AppState<PgStorage>) -> Step {
    let Some(media) = &state.media else {
        return Ok(Ran {
            more: false,
            idle: true,
        });
    };
    let ports = state
        .public_ports()
        .await
        .map_err(|_| "no connection".to_string())?;
    let swept = hypha_media::sweep_abandoned_uploads(
        &hypha_core::Ctx::system(),
        hypha_media::HasAssets::assets(&ports),
        &media.blobs,
        &state.clock,
        &media.settings,
    )
    .await
    .map_err(|e| e.to_string())?;
    if swept > 0 {
        tracing::info!(deleted = swept, "media: swept abandoned uploads");
    }
    Ok(Ran {
        more: false,
        idle: swept == 0,
    })
}

/// Deletes what's past keeping (`[jobs.prune]`): events, idempotency keys, and sessions
/// that ended.
async fn prune(state: &AppState<PgStorage>) -> Step {
    let ports = state
        .public_ports()
        .await
        .map_err(|_| "no connection".to_string())?;
    let events = prune_events(&ports, &state.clock, &state.config.dispatch)
        .await
        .map_err(|e| e.to_string())?;
    let keys = prune_idempotency_keys(&ports, &state.clock, &state.config.dispatch)
        .await
        .map_err(|e| e.to_string())?;
    let sessions = prune_sessions(&ports, &state.clock, &state.config.dispatch)
        .await
        .map_err(|e| e.to_string())?;
    if events.done + keys.done + sessions.done > 0 {
        tracing::info!(
            events = events.done,
            idempotency_keys = keys.done,
            sessions = sessions.done,
            "jobs: pruned"
        );
    }
    Ok(Ran {
        more: events.more || keys.more || sessions.more,
        idle: events.done + keys.done + sessions.done == 0,
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
