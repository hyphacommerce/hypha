//! What a host provides that this crate can't do portably (design note 0026, §1): time,
//! IDs, randomness, CPU-heavy work, work after the response, rate limits, calls to
//! merchant URLs, the media storage's HTTP, sending email, and a signal after each
//! write. The native
//! host (`hypha-host-axum`) gives Tokio's and the operating system's; a Workers host
//! gives the runtime's (`Date.now()`, `crypto.getRandomValues`, `fetch`, `waitUntil`).
//!
//! Each is a small wrapper around a shared function whose futures are boxed and
//! `Send`. That keeps one concrete `AppState` per storage and one `Media` type, and the
//! compiler still proves every handler's future `Send` (a boxed `Send` future is). The
//! repositories stay statically dispatched (CLAUDE.md); only these host capabilities are
//! boxed, as `PgStore` already takes its clock.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use hypha_core::{Clock, IdGenerator, Random, Timestamp};
use hypha_notifications::{EmailSender, OutgoingEmail, SendError, Sent};
use hypha_store_s3::{HttpClient, HttpRequest, HttpResponse};
use uuid::Uuid;

use crate::outbound::{CallError, OutboundCall, OutboundPolicy, SignedCall, prepare};
use crate::rate_limit::{Decision, RateLimiter, Rule};

/// A boxed future that can move between threads, which a host's capability returns.
pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

/// The host's clock: `SystemTime` natively, `Date.now()` on Workers.
#[derive(Clone)]
pub struct HostClock(Arc<dyn Clock + Send + Sync>);

impl HostClock {
    pub fn new(clock: impl Clock + Send + Sync + 'static) -> Self {
        Self(Arc::new(clock))
    }

    /// The same clock, for what takes a shared one (`PgStore::new`).
    pub fn shared(&self) -> Arc<dyn Clock + Send + Sync> {
        Arc::clone(&self.0)
    }
}

impl Clock for HostClock {
    fn now(&self) -> Timestamp {
        self.0.now()
    }
}

/// The host's ID generator: UUIDv7 from its clock and randomness.
#[derive(Clone)]
pub struct HostIds(Arc<dyn IdGenerator + Send + Sync>);

impl HostIds {
    pub fn new(ids: impl IdGenerator + Send + Sync + 'static) -> Self {
        Self(Arc::new(ids))
    }
}

impl IdGenerator for HostIds {
    fn new_uuid(&self) -> Uuid {
        self.0.new_uuid()
    }
}

/// Random bytes for tokens, salts and nonces: the operating system's generator
/// natively, `crypto.getRandomValues` on Workers.
#[derive(Clone)]
pub struct HostRandom(Arc<FillRandom>);

/// How a host fills a buffer with random bytes.
type FillRandom = dyn Fn(&mut [u8]) + Send + Sync;

impl HostRandom {
    pub fn new(fill: impl Fn(&mut [u8]) + Send + Sync + 'static) -> Self {
        Self(Arc::new(fill))
    }
}

impl Random for HostRandom {
    fn fill(&self, bytes: &mut [u8]) {
        (self.0)(bytes);
    }
}

/// A job for `Offload`: some CPU-heavy work, run to completion.
pub type Job = Box<dyn FnOnce() + Send + 'static>;

/// How a host runs a `Job`.
type RunJob = dyn Fn(Job) -> BoxFuture<Result<(), String>> + Send + Sync;

/// Why offloaded work gave no result.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OffloadError {
    /// The host couldn't run it (natively: its blocking thread panicked or was
    /// cancelled); the host's own words.
    #[error("the host couldn't run the work: {0}")]
    Host(String),
    /// The host returned without running it.
    #[error("the work didn't finish")]
    Unfinished,
}

impl hypha_core::ErrorCode for OffloadError {
    fn code(&self) -> &'static str {
        match self {
            OffloadError::Host(_) => "offload_failed",
            OffloadError::Unfinished => "offload_unfinished",
        }
    }
}

/// Where CPU-heavy work runs (hashing a password takes tens of milliseconds): Tokio's
/// blocking threads natively, so it doesn't stall the requests on that worker; inline
/// on Workers, where each request has its own isolate.
#[derive(Clone)]
pub struct Offload(Arc<RunJob>);

impl Offload {
    pub fn new(run: impl Fn(Job) -> BoxFuture<Result<(), String>> + Send + Sync + 'static) -> Self {
        Self(Arc::new(run))
    }

    /// Runs the work where it's called.
    pub fn inline() -> Self {
        Self::new(|job| {
            Box::pin(async move {
                job();
                Ok(())
            })
        })
    }

    /// Runs `work` and gives back its result.
    pub async fn run<T: Send + 'static>(
        &self,
        work: impl FnOnce() -> T + Send + 'static,
    ) -> Result<T, OffloadError> {
        let slot: Arc<Mutex<Option<T>>> = Arc::new(Mutex::new(None));
        let out = Arc::clone(&slot);
        (self.0)(Box::new(move || {
            let value = work();
            *out.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(value);
        }))
        .await
        .map_err(OffloadError::Host)?;
        let value = slot
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        value.ok_or(OffloadError::Unfinished)
    }
}

/// Work that runs after its response went out (a password reset, a buyer's email):
/// a few slots natively (`hypha-host-axum`), `waitUntil` on Workers. `spawn` says
/// whether it was taken; when not, nothing runs.
#[derive(Clone)]
pub struct HostBackground(Arc<dyn Fn(BoxFuture<()>) -> bool + Send + Sync>);

impl HostBackground {
    pub fn new(spawn: impl Fn(BoxFuture<()>) -> bool + Send + Sync + 'static) -> Self {
        Self(Arc::new(spawn))
    }

    pub fn spawn(&self, work: impl Future<Output = ()> + Send + 'static) -> bool {
        (self.0)(Box::pin(work))
    }
}

/// The host's rate limiter (design note 0009): counts in memory natively, Cloudflare's
/// Rate Limiting binding on Workers.
#[derive(Clone)]
pub struct HostLimiter(Arc<CheckLimit>);

/// How a host checks a limit.
type CheckLimit = dyn Fn(Rule, String) -> BoxFuture<Decision> + Send + Sync;

impl HostLimiter {
    pub fn new(
        check: impl Fn(Rule, String) -> BoxFuture<Decision> + Send + Sync + 'static,
    ) -> Self {
        Self(Arc::new(check))
    }
}

impl RateLimiter for HostLimiter {
    async fn check(&self, rule: Rule, key: &str) -> Decision {
        (self.0)(rule, key.to_string()).await
    }
}

/// Calls to merchant-configured URLs (rate apps, webhooks), signed and checked by
/// `outbound::prepare`: the host sends them, natively only to public addresses
/// (`outbound.allow_private` aside), and reads at most `outbound::MAX_ANSWER_BYTES`.
#[derive(Clone)]
pub struct HostCalls(Arc<SendCall>);

/// How a host sends a call.
type SendCall = dyn Fn(OutboundCall) -> BoxFuture<Result<Vec<u8>, CallError>> + Send + Sync;

impl HostCalls {
    pub fn new(
        send: impl Fn(OutboundCall) -> BoxFuture<Result<Vec<u8>, CallError>> + Send + Sync + 'static,
    ) -> Self {
        Self(Arc::new(send))
    }

    pub async fn send(&self, call: OutboundCall) -> Result<Vec<u8>, CallError> {
        (self.0)(call).await
    }

    /// Signs `call` and checks its URL under `policy` (`outbound::prepare`), then sends
    /// it: how rate requests and webhooks go out.
    pub async fn send_signed(
        &self,
        policy: OutboundPolicy,
        call: SignedCall<'_>,
    ) -> Result<Vec<u8>, CallError> {
        self.send(prepare(policy, call)?).await
    }
}

/// The HTTP the media storage's S3 client sends with (`hypha_store_s3::HttpClient`).
#[derive(Clone)]
pub struct HostHttp(Arc<SendRequest>);

/// How a host sends the media storage's requests.
type SendRequest = dyn Fn(HttpRequest) -> BoxFuture<Result<HttpResponse, String>> + Send + Sync;

impl HostHttp {
    pub fn new(
        send: impl Fn(HttpRequest) -> BoxFuture<Result<HttpResponse, String>> + Send + Sync + 'static,
    ) -> Self {
        Self(Arc::new(send))
    }
}

impl HttpClient for HostHttp {
    async fn send(&self, request: HttpRequest) -> Result<HttpResponse, String> {
        (self.0)(request).await
    }
}

/// The email sender the operator picked (`email.sender`, design note 0028, §5): the log
/// (`crate::email::LogSender`), SMTP (native only) or Resend, each host building its
/// own. One concrete `EmailSender` for this crate and `hypha-mail`, whichever it is.
#[derive(Clone)]
pub struct HostMail {
    name: &'static str,
    send: Arc<SendMail>,
}

/// How a host sends an email.
type SendMail = dyn Fn(OutgoingEmail) -> BoxFuture<Result<Sent, SendError>> + Send + Sync;

impl HostMail {
    /// `name` is the sender's (`EmailSender::name`), recorded on each message.
    pub fn new(
        name: &'static str,
        send: impl Fn(OutgoingEmail) -> BoxFuture<Result<Sent, SendError>> + Send + Sync + 'static,
    ) -> Self {
        Self {
            name,
            send: Arc::new(send),
        }
    }
}

impl std::fmt::Debug for HostMail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("HostMail").field(&self.name).finish()
    }
}

impl EmailSender for HostMail {
    fn name(&self) -> &'static str {
        self.name
    }

    async fn send(&self, email: &OutgoingEmail) -> Result<Sent, SendError> {
        (self.send)(email.clone()).await
    }
}

/// Called when a write gives the dispatcher work: the request's ports call it after
/// committing events or retrying a delivery (`hypha_core::SignalsDispatch`, set by
/// `AppState::public_ports`), so a write that recorded none (a cart's) wakes nothing.
/// Natively it wakes the dispatcher (design note 0025, §6); on Workers it marks the
/// event, whose dispatch step then runs in `waitUntil` after the response (0026, W6).
#[derive(Clone)]
pub struct AfterWrites(hypha_core::DispatchHook);

impl AfterWrites {
    pub fn new(notify: impl Fn() + Send + Sync + 'static) -> Self {
        Self(Arc::new(notify))
    }

    /// Does nothing (a host without jobs, tests).
    pub fn none() -> Self {
        Self::new(|| {})
    }

    pub fn notify(&self) {
        (self.0)();
    }

    /// The hook, for the ports (`hypha_core::SignalsDispatch`).
    pub fn hook(&self) -> hypha_core::DispatchHook {
        Arc::clone(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offloaded_work_gives_back_its_result() {
        let offload = Offload::inline();
        assert_eq!(pollster::block_on(offload.run(|| 6 * 7)), Ok(42));
    }

    #[test]
    fn a_host_that_runs_nothing_reports_it() {
        let refusing = Offload::new(|_job| Box::pin(async { Err("no threads".to_string()) }));
        assert_eq!(
            pollster::block_on(refusing.run(|| 1)),
            Err(OffloadError::Host("no threads".to_string()))
        );
    }

    #[test]
    fn a_job_that_is_dropped_unrun_is_an_error() {
        let dropping = Offload::new(|_job| Box::pin(async { Ok(()) }));
        assert_eq!(
            pollster::block_on(dropping.run(|| 1)),
            Err(OffloadError::Unfinished)
        );
    }
}
