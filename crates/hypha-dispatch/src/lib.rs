//! Delivering domain events (design note 0016, sections 5 to 7): fanning the outbox out
//! to destinations, running the due deliveries with retries, the in-process handlers, and
//! webhooks to apps (through the host's `WebhookSender`).
//!
//! A workflow crate: handlers compose domains (cleaning up after a removed variant
//! touches pricing and inventory), like checkout. Everything here is a plain function of
//! ports and a clock, with no runtime: a host calls them from a Tokio interval, a Workers
//! Cron trigger or EventBridge (EV10).
//!
//! The dispatcher must not hold a database connection while a destination runs (EV9),
//! so delivering is three calls a host makes with ports it gets for each: `lease_due`
//! (short), `run` per delivery (each its own ports), then `record` (short).
//! `deliver_due` does all three with one set of ports, for tests and hosts with a
//! connection per invocation anyway.

pub mod handlers;
pub mod jobs;
pub mod schedule;

pub use handlers::{HANDLERS, Handler, destinations};
pub use jobs::{
    BATCH, DispatchPorts, Progress, WebhookSender, deliver_due, fan_out, lease_due, prune_events,
    prune_idempotency_keys, prune_sessions, record, run,
};
pub use schedule::{MAX_ATTEMPTS, next_attempt};
