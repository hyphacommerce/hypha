//! The background jobs' steps (design note 0016, section 7): each does a bounded amount
//! of work and says whether there's more and whether it found any (`Ran`). The hosts
//! run them: `hypha-host-axum` on timers (its `runner`), woken after writes; a Workers
//! host from Cron triggers and `waitUntil` (design note 0026, W6). The steps are the
//! domain crates' plain functions (`hypha_dispatch::fan_out`, …), so both run the same.

use std::time::Duration;

use hypha_core::{Clock, Outcome, Progress, Timestamp};
use hypha_dispatch::{
    Prepared, fan_out, handle, has_time, lease_due, prepare, prune_events, prune_idempotency_keys,
    prune_sessions, record, send,
};
use hypha_notifications::{LONGEST_SEND, NotificationStatus};

use crate::state::{AppState, Storage};

/// What a step did: whether there's more to do right away, whether it found anything
/// at all (an idle step makes the job wait longer), and when work it couldn't finish is
/// due again (a failed search rebuild, under its lease): a host that runs jobs only
/// when there's work records it (design note 0026, W7). Timers ignore it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ran {
    pub more: bool,
    pub idle: bool,
    pub retry_at: Option<Timestamp>,
}

impl Ran {
    pub fn of(progress: Progress) -> Self {
        Self {
            more: progress.more,
            idle: progress.done == 0,
            retry_at: None,
        }
    }
}

/// A step's result; `Err` when the step as a whole couldn't run.
pub type Step = Result<Ran, String>;

/// Fans out new events, then runs the due deliveries. The lease and the outcomes each
/// take a connection briefly; each delivery gets ports to load its app or run its
/// handler, and lets them go before a webhook is sent, so no connection waits on another
/// server (design note 0016, EV9). A host whose connections aren't pooled keeps one for
/// the step instead (`Storage::KEEPS_PORTS_ACROSS_DELIVERIES`). Deliveries the lease can't cover any more are left
/// for the next lease (`has_time`).
pub async fn dispatch<S: Storage>(state: &AppState<S>) -> Step {
    dispatch_within(state, None).await
}

/// Room left at the end of a deadline for recording the outcomes.
const RECORD_MARGIN: Duration = Duration::from_secs(5);

/// `dispatch` for a host that cuts its work off at `deadline` (Workers' `waitUntil`,
/// about 30 seconds after the response; design note 0026, W6): no delivery starts that
/// couldn't finish, its webhook's own timeout included, and be recorded by then. The
/// deliveries left keep their lease until it ends, then the next lease (a cron's) takes
/// them; nothing is sent twice.
pub async fn dispatch_by<S: Storage>(state: &AppState<S>, deadline: Timestamp) -> Step {
    dispatch_within(state, Some(deadline)).await
}

async fn dispatch_within<S: Storage>(state: &AppState<S>, deadline: Option<Timestamp>) -> Step {
    // Whether work taking `takes` can start now and still be recorded by the deadline.
    let fits = |takes: Duration| {
        deadline.is_none_or(|by| state.clock.now().plus(takes + RECORD_MARGIN) <= by)
    };
    let no_connection = |_| "no connection".to_string();
    // Kept for the whole step where the host says that's cheaper (`Storage::
    // KEEPS_PORTS_ACROSS_DELIVERIES`); otherwise each part gets its own and lets it go.
    let held = match S::KEEPS_PORTS_ACROSS_DELIVERIES {
        true => Some(state.public_ports().await.map_err(no_connection)?),
        false => None,
    };
    let (fanned, leased) = {
        let fresh;
        let ports = match &held {
            Some(ports) => ports,
            None => {
                fresh = state.public_ports().await.map_err(no_connection)?;
                &fresh
            }
        };
        let fanned = fan_out(
            ports,
            &state.clock,
            &state.config.dispatch,
            state.storage.handlers(),
        )
        .await
        .map_err(|e| e.to_string())?;
        let leased = lease_due(ports, &state.clock, &state.config.dispatch)
            .await
            .map_err(|e| e.to_string())?;
        (fanned, leased)
    };
    let mut outcomes = Vec::with_capacity(leased.len());
    for delivery in leased
        .iter()
        .take_while(|d| has_time(d, &state.clock) && fits(Duration::ZERO))
    {
        let prepared = {
            let fresh;
            let ports = match &held {
                Some(ports) => ports,
                None => {
                    fresh = state.public_ports().await.map_err(no_connection)?;
                    &fresh
                }
            };
            match prepare(ports, &state.clock, &state.config.dispatch, delivery).await {
                Prepared::Handler => Prepared::Done(
                    handle(
                        ports,
                        &state.clock,
                        &state.config.dispatch,
                        state.storage.handlers(),
                        delivery,
                    )
                    .await,
                ),
                other => other,
            }
        };
        let outcome = match prepared {
            // Too slow to send before the deadline: left under its lease for a later run
            // (whose lease it ends up in), while the deliveries behind it still go.
            Prepared::Webhook(app) if !fits(webhook_time(&app)) => continue,
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
    // When this step's unfinished work is due again: a retry's time, or the lease's end
    // of a delivery it didn't start (out of time); a host that runs jobs only when
    // there's work records it (design note 0026, W7).
    let retries = outcomes.iter().filter_map(|o| match &o.outcome {
        hypha_core::DeliveryOutcome::Retry { at, .. } => Some(*at),
        _ => None,
    });
    let unstarted = leased
        .iter()
        .filter(|d| !outcomes.iter().any(|o| o.delivery == d.id))
        .map(|d| d.leased_until);
    let retry_at = retries.chain(unstarted).min();
    if !outcomes.is_empty() {
        let fresh;
        let ports = match &held {
            Some(ports) => ports,
            None => {
                fresh = state.public_ports().await.map_err(no_connection)?;
                &fresh
            }
        };
        record(ports, &outcomes).await.map_err(|e| e.to_string())?;
    }
    // A step that did work may have marked products for search (its handlers): the
    // runner wakes search sync then.
    let idle = fanned.done == 0 && leased.is_empty();
    Ok(Ran {
        more: fanned.more || leased.len() >= state.config.dispatch.batch as usize,
        idle,
        retry_at,
    })
}

/// The longest `app`'s webhook may take: its timeout.
fn webhook_time(app: &hypha_apps::App) -> Duration {
    app.webhook().map_or(Duration::ZERO, |w| {
        Duration::from_millis(u64::from(w.timeout_ms))
    })
}

/// Sends a batch of the queued emails (design note 0028, §4): leases what's due, lets
/// the connection go, sends each (a provider call of up to `LONGEST_SEND`), then takes a
/// connection to record the outcomes, as `dispatch` delivers webhooks (a host whose
/// connections aren't pooled keeps one, `Storage::KEEPS_PORTS_ACROSS_DELIVERIES`).
/// Messages the lease can't cover any more are left for the next lease.
pub async fn send_notifications<S: Storage>(state: &AppState<S>) -> Step {
    send_notifications_within(state, None).await
}

/// `send_notifications` for a host that cuts its work off at `deadline` (Workers'
/// `waitUntil`): no send starts that couldn't finish and be recorded by then. The
/// messages left keep their lease until it ends, then the next lease takes them.
pub async fn send_notifications_by<S: Storage>(state: &AppState<S>, deadline: Timestamp) -> Step {
    send_notifications_within(state, Some(deadline)).await
}

async fn send_notifications_within<S: Storage>(
    state: &AppState<S>,
    deadline: Option<Timestamp>,
) -> Step {
    let settings = &state.config.send_notifications;
    let fits = |takes: Duration| {
        deadline.is_none_or(|by| state.clock.now().plus(takes + RECORD_MARGIN) <= by)
    };
    let no_connection = |_| "no connection".to_string();
    let held = match S::KEEPS_PORTS_ACROSS_DELIVERIES {
        true => Some(state.public_ports().await.map_err(no_connection)?),
        false => None,
    };
    let leased = {
        let fresh;
        let ports = match &held {
            Some(ports) => ports,
            None => {
                fresh = state.public_ports().await.map_err(no_connection)?;
                &fresh
            }
        };
        hypha_mail::lease_due(ports, &state.clock, settings)
            .await
            .map_err(|e| e.to_string())?
    };
    let mut attempts = Vec::with_capacity(leased.len());
    for message in &leased {
        // Out of lease or out of the host's time: left under its lease for a later run,
        // and so are the ones behind it (they'd have less time still).
        if !hypha_mail::has_time(message, &state.clock) || !fits(LONGEST_SEND) {
            break;
        }
        attempts.push(hypha_mail::send_leased(&*state.mail, message).await);
    }
    let unstarted = leased
        .iter()
        .skip(attempts.len())
        .filter_map(|n| n.lease_until());
    let mut retry_at = unstarted.min();
    if !attempts.is_empty() {
        let fresh;
        let ports = match &held {
            Some(ports) => ports,
            None => {
                fresh = state.public_ports().await.map_err(no_connection)?;
                &fresh
            }
        };
        for (message, attempt) in leased.iter().cloned().zip(attempts) {
            let id = message.id();
            match hypha_mail::record_attempt(ports, &state.clock, settings, message, attempt).await
            {
                Ok(recorded) => {
                    match recorded.status() {
                        NotificationStatus::Failed => tracing::warn!(
                            notification = %id,
                            template = recorded.template().as_str(),
                            "email: failed for good: {}",
                            recorded.error().unwrap_or_default()
                        ),
                        NotificationStatus::Queued => tracing::info!(
                            notification = %id,
                            "email: the provider is unavailable, tried again later: {}",
                            recorded.error().unwrap_or_default()
                        ),
                        _ => {}
                    }
                    retry_at = earliest(retry_at, recorded.next_attempt_at());
                }
                // Changed since it was leased (its lease ran out and another run took
                // it): that run records it.
                Err(e) => tracing::warn!(notification = %id, "email: not recorded: {e}"),
            }
        }
    }
    Ok(Ran {
        more: leased.len() >= settings.batch as usize,
        idle: leased.is_empty(),
        retry_at,
    })
}

fn earliest(a: Option<Timestamp>, b: Option<Timestamp>) -> Option<Timestamp> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

/// Rebuilds a batch of the search documents events marked (design note 0019, section
/// 5). One connection for the step: the built-in engine is the database.
pub async fn search_sync<S: Storage>(state: &AppState<S>) -> Step {
    let ports = state
        .public_ports()
        .await
        .map_err(|_| "no connection".to_string())?;
    let now = state.clock.now();
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
    // A failed product's mark stays queued under this step's lease, free again when it
    // ends.
    let retry_at = (!synced.failed.is_empty()).then(|| now.plus(state.config.search_sync.lease));
    Ok(Ran {
        retry_at,
        ..Ran::of(synced.progress)
    })
}

/// Deletes uploads abandoned for over `media.abandoned_after` (design note 0013,
/// section 3).
pub async fn media_sweep<S: Storage>(state: &AppState<S>) -> Step {
    let Some(media) = &state.media else {
        return Ok(Ran {
            more: false,
            idle: true,
            retry_at: None,
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
        retry_at: None,
    })
}

/// Deletes what's past keeping (`[jobs.prune]`): events, idempotency keys, sessions
/// that ended, and the extension call log (design note 0029).
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
    let calls = hypha_extensions::prune_calls(
        &ports,
        &state.clock,
        &state.config.extensions,
        state.config.dispatch.batch,
    )
    .await
    .map_err(|e| e.to_string())?;
    let done = events.done + keys.done + sessions.done + calls.done;
    if done > 0 {
        tracing::info!(
            events = events.done,
            idempotency_keys = keys.done,
            sessions = sessions.done,
            extension_calls = calls.done,
            "jobs: pruned"
        );
    }
    Ok(Ran {
        more: events.more || keys.more || sessions.more || calls.more,
        idle: done == 0,
        retry_at: None,
    })
}

/// When dispatch or the send job next has work (`OutboxRepo::next_due`,
/// `NotificationRepo::next_due`), for a host that runs the jobs only when there's some
/// (design note 0026, W7).
pub async fn next_due<S: Storage>(state: &AppState<S>) -> Result<Option<Timestamp>, String> {
    use hypha_core::{HasOutbox, OutboxRepo};
    use hypha_notifications::{HasNotifications, NotificationRepo};
    let ports = state
        .public_ports()
        .await
        .map_err(|_| "no connection".to_string())?;
    let events = ports.outbox().next_due().await.map_err(|e| e.to_string())?;
    let emails = ports
        .notifications()
        .next_due()
        .await
        .map_err(|e| e.to_string())?;
    Ok(earliest(events, emails))
}

/// How many search targets are queued, for a host whose runs don't follow each other on
/// a timer (design note 0026, W7): a mark marked again while another run held it is left
/// for the next run, which on Workers may be an hour away.
pub async fn search_queued<S: Storage>(state: &AppState<S>) -> Result<u64, String> {
    use hypha_search_index::{HasSearchQueue, SearchQueue};
    let ports = state
        .public_ports()
        .await
        .map_err(|_| "no connection".to_string())?;
    ports
        .search_queue()
        .queued()
        .await
        .map_err(|e| e.to_string())
}
