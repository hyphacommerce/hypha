//! `call::<P>`: what it sends, how it judges each answer, and what it records.

use std::{cell::RefCell, time::Duration};

use hypha_core::{
    AppKey, Clock, ExtensionCallId, IdGenerator, Page, PageRequest, RepoError, Timestamp,
};
use hypha_extensions::{
    Answer, CallOutcome, EXTENSION_HEADER, EXTENSION_VERSION_HEADER, Endpoint, ExtensionCall,
    ExtensionCallRepo, ExtensionCaller, ExtensionError, ExtensionPoint, ExtensionSettings,
    IDEMPOTENCY_KEY_HEADER, MAX_ANSWER_BYTES, OnFailure, OutgoingCall, TransportError, call,
    prune_calls,
};
use hypha_testkit::{FixedClock, InMemoryExtensionCallRepo, InMemoryPorts, SequentialIds};
use pollster::block_on;
use serde::{Deserialize, Serialize};

/// A point whose answers must be even numbers.
struct Echo;

#[derive(Serialize)]
struct Ask {
    n: u32,
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
struct Reply {
    n: u32,
}

impl ExtensionPoint for Echo {
    const NAME: &'static str = "test.echo";
    const VERSION: u16 = 3;
    const TIMEOUT: Duration = Duration::from_millis(1500);
    const ON_FAILURE: OnFailure = OnFailure::FailClosed;
    const SCOPE: hypha_core::Scope = hypha_core::Scope::OrdersRead;
    const READ_ONLY: bool = true;
    type Request = Ask;
    type Response = Reply;

    fn check(_: &Ask, reply: &Reply) -> Result<(), String> {
        if reply.n.is_multiple_of(2) {
            Ok(())
        } else {
            Err(format!("{} is odd", reply.n))
        }
    }
}

/// One call as the fake app saw it: URL, secret, headers, body and timeout.
type Seen = (
    String,
    String,
    Vec<(&'static str, String)>,
    Vec<u8>,
    Duration,
);

/// What the fake app saw, and what it answers.
struct FakeApp {
    answer: Result<Answer, TransportError>,
    seen: RefCell<Vec<Seen>>,
}

impl FakeApp {
    fn answering(answer: Result<Answer, TransportError>) -> Self {
        Self {
            answer,
            seen: RefCell::default(),
        }
    }

    fn ok(status: u16, body: &str) -> Self {
        Self::answering(Ok(Answer {
            status,
            body: body.as_bytes().to_vec(),
        }))
    }
}

impl ExtensionCaller for FakeApp {
    async fn send(&self, call: OutgoingCall<'_>) -> Result<Answer, TransportError> {
        self.seen.borrow_mut().push((
            call.url.to_string(),
            call.secret.to_string(),
            call.headers,
            call.body,
            call.timeout,
        ));
        self.answer.clone()
    }
}

/// The clock and IDs `call` takes as one.
struct Env {
    clock: FixedClock,
    ids: SequentialIds,
}

impl Clock for Env {
    fn now(&self) -> Timestamp {
        self.clock.now()
    }
}

impl IdGenerator for Env {
    fn new_uuid(&self) -> uuid::Uuid {
        self.ids.new_uuid()
    }
}

fn env() -> Env {
    Env {
        clock: FixedClock::at(Timestamp::from_unix_micros(1_800_000_000_000_000)),
        ids: SequentialIds::default(),
    }
}

fn app() -> AppKey {
    AppKey::new("app_echo").unwrap()
}

fn run(
    fake: &FakeApp,
    log: &InMemoryExtensionCallRepo,
    timeout: Option<Duration>,
) -> Result<Reply, ExtensionError> {
    let key = app();
    let extra = [("Hypha-Request-Id", "rreq_1".to_string())];
    let endpoint = Endpoint {
        app: &key,
        url: "https://echo.example/x",
        secret: "s3cret",
        timeout,
        headers: &extra,
    };
    block_on(call::<Echo>(
        fake,
        log,
        &env(),
        &endpoint,
        &Ask { n: 4 },
        "pay_1",
        Some("cart_9"),
    ))
}

#[test]
fn a_call_sends_the_body_and_the_points_headers() {
    let fake = FakeApp::ok(200, r#"{"n": 8}"#);
    let log = InMemoryExtensionCallRepo::default();
    assert_eq!(run(&fake, &log, None), Ok(Reply { n: 8 }));
    let seen = fake.seen.borrow();
    let (url, secret, headers, body, timeout) = &seen[0];
    assert_eq!(
        (url.as_str(), secret.as_str()),
        ("https://echo.example/x", "s3cret")
    );
    assert_eq!(body, br#"{"n":4}"#);
    assert_eq!(*timeout, Echo::TIMEOUT);
    assert_eq!(
        *headers,
        vec![
            (EXTENSION_HEADER, "test.echo".to_string()),
            (EXTENSION_VERSION_HEADER, "3".to_string()),
            (IDEMPOTENCY_KEY_HEADER, "pay_1".to_string()),
            ("Hypha-Request-Id", "rreq_1".to_string()),
        ]
    );
}

#[test]
fn an_answer_is_recorded_as_ok() {
    let fake = FakeApp::ok(201, r#"{"n": 8}"#);
    let log = InMemoryExtensionCallRepo::default();
    run(&fake, &log, None).unwrap();
    let calls = log.all();
    assert_eq!(calls.len(), 1);
    let recorded = &calls[0];
    assert_eq!(recorded.app, app());
    assert_eq!(
        (recorded.point.as_str(), recorded.version),
        ("test.echo", 3)
    );
    assert_eq!(recorded.outcome, CallOutcome::Ok);
    assert_eq!(recorded.status, Some(201));
    assert_eq!(recorded.error, None);
    assert_eq!(recorded.correlation.as_deref(), Some("cart_9"));
    assert_eq!(recorded.started_at, env().now());
}

#[test]
fn the_apps_own_timeout_wins_over_the_points() {
    let fake = FakeApp::ok(200, r#"{"n": 2}"#);
    let log = InMemoryExtensionCallRepo::default();
    run(&fake, &log, Some(Duration::from_millis(300))).unwrap();
    assert_eq!(fake.seen.borrow()[0].4, Duration::from_millis(300));
}

#[test]
fn a_status_other_than_2xx_fails_with_the_apps_code() {
    let fake = FakeApp::ok(401, r#"{"code":"invalid_signature","error":"no"}"#);
    let log = InMemoryExtensionCallRepo::default();
    assert_eq!(
        run(&fake, &log, None),
        Err(ExtensionError::Unavailable {
            app: app(),
            point: "test.echo",
            reason: "answered HTTP 401: invalid_signature".to_string()
        })
    );
    let recorded = &log.all()[0];
    assert_eq!(
        (recorded.outcome, recorded.status),
        (CallOutcome::Failed, Some(401))
    );
    assert_eq!(
        recorded.error.as_deref(),
        Some("answered HTTP 401: invalid_signature")
    );
}

#[test]
fn a_redirect_is_a_failure_too() {
    let fake = FakeApp::ok(302, "");
    let log = InMemoryExtensionCallRepo::default();
    assert!(matches!(
        run(&fake, &log, None),
        Err(ExtensionError::Unavailable { .. })
    ));
    assert_eq!(log.all()[0].error.as_deref(), Some("answered HTTP 302"));
}

#[test]
fn a_body_that_isnt_the_response_is_invalid() {
    let fake = FakeApp::ok(200, r#"{"m": 1}"#);
    let log = InMemoryExtensionCallRepo::default();
    let result = run(&fake, &log, None);
    assert!(
        matches!(&result, Err(ExtensionError::Invalid { reason, .. })
            if reason.starts_with("not a test.echo response: missing field `n`")),
        "{result:?}"
    );
    assert_eq!(
        (log.all()[0].outcome, log.all()[0].status),
        (CallOutcome::Invalid, Some(200))
    );
}

#[test]
fn an_answer_breaking_the_points_rules_is_invalid() {
    let fake = FakeApp::ok(200, r#"{"n": 3}"#);
    let log = InMemoryExtensionCallRepo::default();
    assert_eq!(
        run(&fake, &log, None),
        Err(ExtensionError::Invalid {
            app: app(),
            point: "test.echo",
            reason: "3 is odd".to_string()
        })
    );
    assert_eq!(log.all()[0].error.as_deref(), Some("3 is odd"));
}

#[test]
fn an_answer_over_the_cap_is_invalid() {
    let huge = format!(r#"{{"n": 2, "pad": "{}"}}"#, "x".repeat(MAX_ANSWER_BYTES));
    let fake = FakeApp::ok(200, &huge);
    let log = InMemoryExtensionCallRepo::default();
    let reason = format!("answered more than {MAX_ANSWER_BYTES} bytes");
    assert_eq!(
        run(&fake, &log, None),
        Err(ExtensionError::Invalid {
            app: app(),
            point: "test.echo",
            reason: reason.clone()
        })
    );
    // A host that stopped reading at the cap says so the same way.
    let capped = FakeApp::answering(Err(TransportError::TooLarge));
    let log = InMemoryExtensionCallRepo::default();
    assert!(matches!(
        run(&capped, &log, None),
        Err(ExtensionError::Invalid { .. })
    ));
    assert_eq!(
        (log.all()[0].outcome, log.all()[0].status),
        (CallOutcome::Invalid, None)
    );
    assert_eq!(log.all()[0].error, Some(reason));
}

#[test]
fn no_answer_in_time_is_a_timeout() {
    let fake = FakeApp::answering(Err(TransportError::Timeout));
    let log = InMemoryExtensionCallRepo::default();
    assert_eq!(
        run(&fake, &log, None),
        Err(ExtensionError::Timeout {
            app: app(),
            point: "test.echo"
        })
    );
    let recorded = &log.all()[0];
    assert_eq!(
        (recorded.outcome, recorded.status),
        (CallOutcome::Timeout, None)
    );
    assert_eq!(recorded.error.as_deref(), Some("no answer within 1500 ms"));
}

#[test]
fn an_unreachable_app_fails_with_what_the_host_says() {
    let fake = FakeApp::answering(Err(TransportError::Unreachable("unreachable".to_string())));
    let log = InMemoryExtensionCallRepo::default();
    assert_eq!(
        run(&fake, &log, None),
        Err(ExtensionError::Unavailable {
            app: app(),
            point: "test.echo",
            reason: "unreachable".to_string()
        })
    );
    assert_eq!(log.all()[0].outcome, CallOutcome::Failed);
}

#[test]
fn every_failure_is_app_unavailable_with_the_app_and_point() {
    use hypha_core::ErrorCode;
    let fake = FakeApp::answering(Err(TransportError::Timeout));
    let log = InMemoryExtensionCallRepo::default();
    let error = run(&fake, &log, None).unwrap_err();
    assert_eq!(error.code(), "app_unavailable");
    assert_eq!((error.app(), error.point()), (&app(), "test.echo"));
}

#[test]
fn each_call_is_recorded_once_with_its_own_id() {
    let fake = FakeApp::ok(200, r#"{"n": 2}"#);
    let log = InMemoryExtensionCallRepo::default();
    let key = app();
    let endpoint = Endpoint {
        app: &key,
        url: "https://echo.example/x",
        secret: "s",
        timeout: None,
        headers: &[],
    };
    let env = env();
    for _ in 0..2 {
        block_on(call::<Echo>(
            &fake,
            &log,
            &env,
            &endpoint,
            &Ask { n: 1 },
            "k",
            None,
        ))
        .unwrap();
    }
    let calls = log.all();
    assert_eq!(calls.len(), 2);
    assert_ne!(calls[0].id, calls[1].id);
    assert_eq!(calls[0].correlation, None);
}

/// A call log whose every write fails.
struct BrokenLog;

impl ExtensionCallRepo for BrokenLog {
    async fn insert(&self, _: &ExtensionCall) -> Result<(), RepoError> {
        Err(RepoError::Backend("the log is down".to_string()))
    }

    async fn list_for_app(
        &self,
        _: &AppKey,
        _: PageRequest<ExtensionCallId>,
    ) -> Result<Page<ExtensionCall>, RepoError> {
        Err(RepoError::Backend("the log is down".to_string()))
    }

    async fn prune(&self, _: Timestamp, _: u32) -> Result<u32, RepoError> {
        Err(RepoError::Backend("the log is down".to_string()))
    }
}

#[test]
fn a_call_that_cant_be_recorded_still_answers() {
    let fake = FakeApp::ok(200, r#"{"n": 8}"#);
    let key = app();
    let endpoint = Endpoint {
        app: &key,
        url: "https://echo.example/x",
        secret: "s",
        timeout: None,
        headers: &[],
    };
    assert_eq!(
        block_on(call::<Echo>(
            &fake,
            &BrokenLog,
            &env(),
            &endpoint,
            &Ask { n: 4 },
            "k",
            None,
        )),
        Ok(Reply { n: 8 })
    );
}

fn recorded_at(started_at: Timestamp, n: u128) -> ExtensionCall {
    ExtensionCall {
        id: ExtensionCallId::from_uuid(uuid::Uuid::from_u128(n)),
        app: app(),
        point: Echo::NAME.to_string(),
        version: Echo::VERSION,
        started_at,
        duration_ms: 10,
        outcome: CallOutcome::Ok,
        status: Some(200),
        error: None,
        correlation: None,
    }
}

#[test]
fn pruning_drops_calls_older_than_the_cutoff_and_keeps_the_rest() {
    let ports = InMemoryPorts::default();
    let clock = env().clock;
    let settings = ExtensionSettings::default();
    let cutoff = clock.now().minus(settings.keep_calls);
    let inside = recorded_at(cutoff, 1);
    let outside = recorded_at(Timestamp::from_unix_micros(cutoff.unix_micros() - 1), 2);
    block_on(ports.extension_calls.insert(&inside)).unwrap();
    block_on(ports.extension_calls.insert(&outside)).unwrap();
    let progress = block_on(prune_calls(&ports, &clock, &settings, 10)).unwrap();
    assert_eq!(progress.done, 1);
    assert_eq!(ports.extension_calls.all(), [inside]);
}
