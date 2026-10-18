use hypha_core::{AppKey, ExtensionCallId, PageLimit, PageRequest, RepoError, Timestamp};
use hypha_extensions::{CallOutcome, ExtensionCall, ExtensionCallRepo, HasExtensionCalls, keys};
use uuid::Uuid;

use super::scenario;

/// Runs every `ExtensionCallRepo` scenario, each on fresh ports from `make`.
pub async fn extension_call_repo<S: HasExtensionCalls>(make: impl AsyncFn() -> S) {
    scenario("extension calls: every field round-trips, an empty one too");
    round_trips(&make().await).await;
    scenario("extension calls: an ID is stored once");
    duplicate_id(&make().await).await;
    scenario("extension calls: an app's calls page newest first, without another app's");
    lists_by_app(&make().await).await;
    scenario("extension calls: prune deletes calls started before a time, up to a limit");
    prunes(&make().await).await;
}

fn app(key: &str) -> AppKey {
    AppKey::new(key).unwrap()
}

fn id(n: u128) -> ExtensionCallId {
    ExtensionCallId::from_uuid(Uuid::from_u128(n))
}

fn call(n: u128, app_key: &str) -> ExtensionCall {
    ExtensionCall {
        id: id(n),
        app: app(app_key),
        point: "shipping.rates".to_string(),
        version: 1,
        started_at: Timestamp::from_unix_micros(1_800_000_000_000_000 + n as i64),
        duration_ms: 0,
        outcome: CallOutcome::Ok,
        status: None,
        error: None,
        correlation: None,
    }
}

async fn page<S: HasExtensionCalls>(
    ports: &S,
    key: &str,
    after: Option<u128>,
    limit: u32,
) -> (Vec<ExtensionCallId>, bool) {
    let page = ports
        .extension_calls()
        .list_for_app(
            &app(key),
            PageRequest {
                after: after.map(id),
                limit: PageLimit::new(limit).unwrap(),
            },
        )
        .await
        .unwrap();
    (page.items.iter().map(|c| c.id).collect(), page.has_more)
}

async fn round_trips<S: HasExtensionCalls>(ports: &S) {
    let full = ExtensionCall {
        point: "payments.start".to_string(),
        version: 2,
        duration_ms: 1234,
        outcome: CallOutcome::Failed,
        status: Some(401),
        error: Some("answered HTTP 401: invalid_signature".to_string()),
        correlation: Some("pay_00000000000000000000000000000009".to_string()),
        ..call(1, "app_stripe")
    };
    let bare = ExtensionCall {
        outcome: CallOutcome::Timeout,
        ..call(2, "app_stripe")
    };
    let invalid = ExtensionCall {
        outcome: CallOutcome::Invalid,
        status: Some(200),
        ..call(3, "app_stripe")
    };
    let repo = ports.extension_calls();
    for c in [&full, &bare, &invalid] {
        repo.insert(c).await.unwrap();
    }
    let found = repo
        .list_for_app(&app("app_stripe"), PageRequest::default())
        .await
        .unwrap();
    assert_eq!(found.items, vec![invalid, bare, full]);
    assert!(!found.has_more);
}

async fn duplicate_id<S: HasExtensionCalls>(ports: &S) {
    let repo = ports.extension_calls();
    repo.insert(&call(1, "app_a")).await.unwrap();
    assert_eq!(
        repo.insert(&call(1, "app_b")).await,
        Err(RepoError::Duplicate(keys::EXTENSION_CALL_ID))
    );
    assert_eq!(page(ports, "app_b", None, 10).await, (vec![], false));
}

async fn lists_by_app<S: HasExtensionCalls>(ports: &S) {
    let repo = ports.extension_calls();
    for n in 1..=5 {
        repo.insert(&call(n, "app_a")).await.unwrap();
    }
    repo.insert(&call(6, "app_b")).await.unwrap();
    assert_eq!(
        page(ports, "app_a", None, 2).await,
        (vec![id(5), id(4)], true)
    );
    assert_eq!(
        page(ports, "app_a", Some(4), 2).await,
        (vec![id(3), id(2)], true)
    );
    assert_eq!(page(ports, "app_a", Some(2), 2).await, (vec![id(1)], false));
    assert_eq!(page(ports, "app_b", None, 10).await, (vec![id(6)], false));
    assert_eq!(page(ports, "app_none", None, 10).await, (vec![], false));
}

async fn prunes<S: HasExtensionCalls>(ports: &S) {
    let repo = ports.extension_calls();
    let at = |n: u128, micros: i64| ExtensionCall {
        started_at: Timestamp::from_unix_micros(micros),
        ..call(n, "app_a")
    };
    for c in [at(1, 1), at(2, 2), at(3, 3), at(4, 10)] {
        repo.insert(&c).await.unwrap();
    }
    let cutoff = Timestamp::from_unix_micros(10);
    assert_eq!(repo.prune(cutoff, 2).await, Ok(2));
    assert_eq!(repo.prune(cutoff, 10).await, Ok(1));
    assert_eq!(repo.prune(cutoff, 10).await, Ok(0));
    assert_eq!(page(ports, "app_a", None, 10).await, (vec![id(4)], false));
}
