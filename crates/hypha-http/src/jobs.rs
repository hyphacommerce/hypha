//! The background jobs' steps (design note 0016, section 7): each does a bounded amount
//! of work and says whether there's more and whether it found any (`Ran`). The hosts
//! run them: `hypha-host-axum` on timers (its `runner`), woken after writes; a Workers
//! host from Cron triggers and `waitUntil` (design note 0026, W6). The steps are the
//! domain crates' plain functions (`hypha_dispatch::fan_out`, …), so both run the same.

use hypha_core::{Outcome, Progress};
use hypha_dispatch::{
    Prepared, fan_out, handle, has_time, lease_due, prepare, prune_events, prune_idempotency_keys,
    prune_sessions, record, send,
};

use crate::{
    handlers::HANDLERS,
    state::{AppState, Storage},
};

/// What a step did: whether there's more to do right away, and whether it found
/// anything at all (an idle step makes the job wait longer).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ran {
    pub more: bool,
    pub idle: bool,
}

impl Ran {
    pub fn of(progress: Progress) -> Self {
        Self {
            more: progress.more,
            idle: progress.done == 0,
        }
    }
}

/// A step's result; `Err` when the step as a whole couldn't run.
pub type Step = Result<Ran, String>;

/// Fans out new events, then runs the due deliveries. The lease and the outcomes each
/// take a connection briefly; each delivery gets ports to load its app or run its
/// handler, and lets them go before a webhook is sent, so no connection waits on another
/// server (design note 0016, EV9). Deliveries the lease can't cover any more are left
/// for the next lease (`has_time`).
pub async fn dispatch<S: Storage>(state: &AppState<S>) -> Step {
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
    // A step that did work may have marked products for search (its handlers): the
    // runner wakes search sync then.
    let idle = fanned.done == 0 && leased.is_empty();
    Ok(Ran {
        more: fanned.more || leased.len() >= state.config.dispatch.batch as usize,
        idle,
    })
}

/// Rebuilds a batch of the search documents events marked (design note 0019, section
/// 5). One connection for the step: the built-in engine is the database.
pub async fn search_sync<S: Storage>(state: &AppState<S>) -> Step {
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
pub async fn media_sweep<S: Storage>(state: &AppState<S>) -> Step {
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
pub async fn prune<S: Storage>(state: &AppState<S>) -> Step {
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
