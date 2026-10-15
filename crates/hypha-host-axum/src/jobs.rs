//! Background jobs (design note 0016, section 7): one runner for everything that runs on
//! a timer rather than for a request. Each job is a step that does a bounded amount of
//! work and says whether there's more; the runner repeats it while there is, then waits
//! its interval. Stopping waits for the steps in flight to finish (`Jobs::stop`).
//!
//! The steps are the domain crates' plain functions (`hypha_dispatch::fan_out`, …), so
//! a Workers host calls the same ones from a Cron trigger instead of this runner.

use std::{future::Future, sync::Arc, time::Duration};

use hypha_dispatch::{
    fan_out, lease_due, prune_events, prune_idempotency_keys, prune_sessions, record, run,
};
use tokio::{sync::watch, task::JoinHandle};

use crate::state::{AppState, PgStorage, SharedState};

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

/// A step's result: whether there's more to do right away.
type Step = Result<bool, String>;

/// Starts the jobs for a Postgres host.
pub fn start(state: SharedState<PgStorage>) -> Jobs {
    let (stop, stopped) = watch::channel(false);
    let mut tasks = Vec::new();

    // Delivering events: fan out new ones and run what's due, every 2 seconds.
    let s = Arc::clone(&state);
    tasks.push(every(
        &stopped,
        "dispatch",
        Duration::from_secs(2),
        move || {
            let s = Arc::clone(&s);
            async move { dispatch(&s).await }
        },
    ));

    // Old events, idempotency keys and ended sessions, every hour.
    let s = Arc::clone(&state);
    tasks.push(every(
        &stopped,
        "prune",
        Duration::from_secs(3600),
        move || {
            let s = Arc::clone(&s);
            async move { prune(&s).await }
        },
    ));

    if state.media.is_some() {
        let s = Arc::clone(&state);
        tasks.push(every(
            &stopped,
            "media_sweep",
            Duration::from_secs(3600),
            move || {
                let s = Arc::clone(&s);
                async move { media_sweep(&s).await }
            },
        ));
    }

    // Idle rate-limit keys, so rotating addresses can't grow memory.
    let s = Arc::clone(&state);
    tasks.push(every(
        &stopped,
        "rate_limits",
        Duration::from_secs(60),
        move || {
            let s = Arc::clone(&s);
            async move {
                s.limiter.forget_idle();
                Ok(false)
            }
        },
    ));

    Jobs { stop, tasks }
}

/// Runs `step` every `interval` (repeating it at once while it reports more) until told
/// to stop. A failing step is logged and tried again next interval.
fn every<F, Fut>(
    stopped: &watch::Receiver<bool>,
    name: &'static str,
    interval: Duration,
    step: F,
) -> JoinHandle<()>
where
    F: Fn() -> Fut + Send + 'static,
    Fut: Future<Output = Step> + Send,
{
    let mut stopped = stopped.clone();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = stopped.changed() => return,
                () = tokio::time::sleep(interval) => {}
            }
            loop {
                match step().await {
                    Ok(true) if !*stopped.borrow() => continue,
                    Ok(_) => break,
                    Err(e) => {
                        tracing::warn!(job = name, "jobs: step failed: {e}");
                        break;
                    }
                }
            }
            if *stopped.borrow() {
                return;
            }
        }
    })
}

/// Fans out new events, then runs the due deliveries. The lease and the outcomes each
/// take a connection briefly; every delivery gets its own ports while it runs, so none is
/// held longer than its handler or webhook needs it (design note 0016, EV9).
pub(crate) async fn dispatch(state: &AppState<PgStorage>) -> Step {
    let no_connection = |_| "no connection".to_string();
    let (fanned, leased) = {
        let ports = state.public_ports().await.map_err(no_connection)?;
        let fanned = fan_out(&ports, &state.clock)
            .await
            .map_err(|e| e.to_string())?;
        let leased = lease_due(&ports, &state.clock)
            .await
            .map_err(|e| e.to_string())?;
        (fanned, leased)
    };
    let mut outcomes = Vec::with_capacity(leased.len());
    for delivery in &leased {
        let ports = state.public_ports().await.map_err(no_connection)?;
        let outcome = run(&ports, &state.clock, &state.webhooks(), delivery).await;
        if let hypha_core::DeliveryOutcome::Failed { error } = &outcome {
            tracing::warn!(
                destination = delivery.destination,
                event = %delivery.event.id,
                "jobs: delivery failed for good: {error}"
            );
        }
        outcomes.push((delivery.id, outcome));
    }
    let ports = state.public_ports().await.map_err(no_connection)?;
    record(&ports, &outcomes).await.map_err(|e| e.to_string())?;
    Ok(fanned.more || leased.len() >= hypha_dispatch::BATCH as usize)
}

/// Deletes uploads abandoned for over a day (design note 0013, section 3).
async fn media_sweep(state: &AppState<PgStorage>) -> Step {
    let Some(media) = &state.media else {
        return Ok(false);
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
    )
    .await
    .map_err(|e| e.to_string())?;
    if swept > 0 {
        tracing::info!(deleted = swept, "media: swept abandoned uploads");
    }
    Ok(false)
}

/// Deletes what's past keeping: events after 30 days, idempotency keys after a day,
/// sessions 30 days after they ended.
async fn prune(state: &AppState<PgStorage>) -> Step {
    let ports = state
        .public_ports()
        .await
        .map_err(|_| "no connection".to_string())?;
    let events = prune_events(&ports, &state.clock)
        .await
        .map_err(|e| e.to_string())?;
    let keys = prune_idempotency_keys(&ports, &state.clock)
        .await
        .map_err(|e| e.to_string())?;
    let sessions = prune_sessions(&ports, &state.clock)
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
    Ok(events.more || keys.more || sessions.more)
}
