//! Apps' extension endpoints (design note 0029, §3): every field round trips, a
//! replace is all at once, an endpoint needs its app and goes with it.

use hypha_apps::{
    App, AppKey, AppRepo, ExtensionEndpoint, ExtensionEndpointRepo, HasApps, HasExtensionEndpoints,
    NewApp, endpoints::keys,
};
use hypha_core::{CallbackUrl, RepoError, Sealed};

use super::scenario;

/// Runs every `ExtensionEndpointRepo` scenario, each on fresh ports from `make`.
pub async fn extension_endpoint_repo<S: HasApps + HasExtensionEndpoints>(
    make: impl AsyncFn() -> S,
) {
    scenario("extension endpoints: replace stores every field, by point; get finds one");
    round_trips(&make().await).await;
    scenario("extension endpoints: a replace replaces all; empty removes all");
    replaces(&make().await).await;
    scenario("extension endpoints: need their app, and go when it does");
    need_their_app(&make().await).await;
}

fn key(k: &str) -> AppKey {
    AppKey::new(k).unwrap()
}

async fn add_app<S: HasApps>(ports: &S, k: &str) {
    let mut app = App::new(NewApp {
        key: key(k),
        id: super::apps::id_of(k),
        name: "Pay".to_string(),
        secret: Sealed::from_bytes(vec![1, 2]),
        webhook: None,
        subscriptions: vec![],
    })
    .unwrap();
    ports.apps().insert(&mut app).await.unwrap();
}

fn endpoint(point: &str, version: u16, url: &str) -> ExtensionEndpoint {
    ExtensionEndpoint {
        point: point.to_string(),
        version,
        url: CallbackUrl::new(url).unwrap(),
    }
}

async fn round_trips<S: HasApps + HasExtensionEndpoints>(ports: &S) {
    add_app(ports, "app_pay").await;
    let repo = ports.extension_endpoints();
    assert_eq!(repo.list_for_app(&key("app_pay")).await, Ok(vec![]));
    let start = endpoint("payments.start", 1, "https://pay.example/hypha/start");
    let capture = endpoint(
        "payments.capture",
        65535,
        "https://pay.example/hypha/capture",
    );
    repo.replace_for_app(&key("app_pay"), &[start.clone(), capture.clone()])
        .await
        .unwrap();
    assert_eq!(
        repo.list_for_app(&key("app_pay")).await,
        Ok(vec![capture, start.clone()])
    );
    assert_eq!(
        repo.get(&key("app_pay"), "payments.start").await,
        Ok(Some(start))
    );
    assert_eq!(repo.get(&key("app_pay"), "search.query").await, Ok(None));
    assert_eq!(
        repo.get(&key("app_other"), "payments.start").await,
        Ok(None)
    );
}

async fn replaces<S: HasApps + HasExtensionEndpoints>(ports: &S) {
    add_app(ports, "app_pay").await;
    add_app(ports, "app_find").await;
    let repo = ports.extension_endpoints();
    let search = endpoint("search.query", 1, "https://find.example/q");
    repo.replace_for_app(&key("app_find"), std::slice::from_ref(&search))
        .await
        .unwrap();
    repo.replace_for_app(
        &key("app_pay"),
        &[endpoint("payments.start", 1, "https://pay.example/a")],
    )
    .await
    .unwrap();
    let moved = endpoint("payments.void", 1, "https://pay.example/v");
    repo.replace_for_app(&key("app_pay"), std::slice::from_ref(&moved))
        .await
        .unwrap();
    assert_eq!(repo.list_for_app(&key("app_pay")).await, Ok(vec![moved]));
    repo.replace_for_app(&key("app_pay"), &[]).await.unwrap();
    assert_eq!(repo.list_for_app(&key("app_pay")).await, Ok(vec![]));
    // Another app's are untouched.
    assert_eq!(repo.list_for_app(&key("app_find")).await, Ok(vec![search]));
}

async fn need_their_app<S: HasApps + HasExtensionEndpoints>(ports: &S) {
    let repo = ports.extension_endpoints();
    let start = endpoint("payments.start", 1, "https://pay.example/a");
    assert_eq!(
        repo.replace_for_app(&key("app_gone"), std::slice::from_ref(&start))
            .await,
        Err(RepoError::Reference(keys::ENDPOINT_APP))
    );
    add_app(ports, "app_pay").await;
    repo.replace_for_app(&key("app_pay"), std::slice::from_ref(&start))
        .await
        .unwrap();
    ports.apps().delete(&key("app_pay")).await.unwrap();
    add_app(ports, "app_pay").await;
    assert_eq!(repo.list_for_app(&key("app_pay")).await, Ok(vec![]));
}
