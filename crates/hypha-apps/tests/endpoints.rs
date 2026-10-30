//! Where an app answers an extension point: an app made by hand where staff set, one
//! installed through OAuth where its manifest says, while its grant has the point's scope.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a failing test panics"
)]
use hypha_apps::{
    App, AppError, AppManifest, AppRepo, CommandError, EndpointInput, ExtensionEndpoint,
    ExtensionEndpointRepo, Grant, InstalledManifest, KnownPoint, NewApp, find_endpoint,
    list_extension_endpoints, set_extension_endpoints,
};
use hypha_core::{Actor, AppId, AppKey, CallbackUrl, Ctx, Scope, Sealed, Timestamp, UserId};
use hypha_testkit::InMemoryPorts;
use pollster::block_on;
use uuid::Uuid;

const START: KnownPoint = KnownPoint {
    name: "payments.start",
    version: 1,
    scope: Scope::PaymentsWrite,
};

fn key(k: &str) -> AppKey {
    AppKey::new(k).unwrap()
}

fn at() -> Timestamp {
    Timestamp::from_unix_micros(1_800_000_000_000_000)
}

fn by_hand(ports: &InMemoryPorts, k: &str) {
    let mut app = App::new(NewApp {
        key: key(k),
        id: AppId::from_uuid(Uuid::from_u128(1)),
        name: "Pay".to_string(),
        secret: Sealed::from_bytes(vec![1]),
        webhook: None,
        subscriptions: vec![],
    })
    .unwrap();
    block_on(ports.apps.insert(&mut app)).unwrap();
}

fn manifest(k: &str) -> AppManifest {
    let document = serde_json::json!({
        "key": k,
        "name": "Pay",
        "version": "1.0.0",
        "app_url": "https://pay.example/base/",
        "redirect_uris": ["/callback"],
        "scopes": ["payments:write", "customer_data:email"],
        "scope_reasons": { "customer_data:email": "receipts" },
        "privacy_url": "/privacy",
        "extension_points": [
            { "point": "payments.start", "version": 1, "url": "start" },
            { "point": "payments.void", "version": 2, "url": "/void" }
        ]
    });
    AppManifest::parse(document.to_string().as_bytes()).unwrap()
}

fn installed(ports: &InMemoryPorts, k: &str, scopes: &[Scope]) -> App {
    let mut app = App::install(
        AppId::from_uuid(Uuid::from_u128(2)),
        Sealed::from_bytes(vec![2]),
        InstalledManifest {
            url: "https://pay.example/hypha-app.json".to_string(),
            manifest: manifest(k),
            fetched_at: at(),
        },
        Grant::new(
            scopes,
            Actor::User(UserId::from_uuid(Uuid::from_u128(9))),
            at(),
        ),
    )
    .unwrap();
    block_on(ports.apps.insert(&mut app)).unwrap();
    app
}

fn url_of(ports: &InMemoryPorts, k: &str, point: KnownPoint) -> Option<String> {
    block_on(find_endpoint(ports, &key(k), point))
        .unwrap()
        .map(|(_, e)| e.url.as_str().to_string())
}

#[test]
fn an_app_made_by_hand_answers_where_staff_set() {
    let ports = InMemoryPorts::default();
    by_hand(&ports, "app_pay");
    assert_eq!(url_of(&ports, "app_pay", START), None);
    let endpoint = ExtensionEndpoint {
        point: "payments.start".to_string(),
        version: 1,
        url: CallbackUrl::new("https://pay.example/start").unwrap(),
    };
    block_on(
        ports
            .extension_endpoints
            .replace_for_app(&key("app_pay"), &[endpoint]),
    )
    .unwrap();
    assert_eq!(
        url_of(&ports, "app_pay", START).as_deref(),
        Some("https://pay.example/start")
    );
}

#[test]
fn an_installed_app_answers_where_its_manifest_says() {
    let ports = InMemoryPorts::default();
    installed(&ports, "app_pay", &[Scope::PaymentsWrite]);
    assert_eq!(
        url_of(&ports, "app_pay", START).as_deref(),
        Some("https://pay.example/base/start")
    );
}

#[test]
fn a_manifests_point_of_another_version_isnt_called() {
    let ports = InMemoryPorts::default();
    installed(&ports, "app_pay", &[Scope::PaymentsWrite]);
    let void = KnownPoint {
        name: "payments.void",
        version: 1,
        scope: Scope::PaymentsWrite,
    };
    assert_eq!(url_of(&ports, "app_pay", void), None);
}

#[test]
fn an_installed_app_without_the_points_scope_isnt_called() {
    let ports = InMemoryPorts::default();
    installed(&ports, "app_pay", &[Scope::CustomerDataEmail]);
    assert_eq!(url_of(&ports, "app_pay", START), None);
}

#[test]
fn an_uninstalled_or_disabled_app_isnt_called() {
    let ports = InMemoryPorts::default();
    let mut app = installed(&ports, "app_pay", &[Scope::PaymentsWrite]);
    app.set_enabled(false);
    block_on(ports.apps.update(&mut app)).unwrap();
    assert_eq!(url_of(&ports, "app_pay", START), None);
    app.set_enabled(true);
    app.uninstall(at()).unwrap();
    block_on(ports.apps.update(&mut app)).unwrap();
    assert_eq!(url_of(&ports, "app_pay", START), None);
}

#[test]
fn an_installed_apps_endpoints_are_its_manifests_and_not_set_by_staff() {
    let ports = InMemoryPorts::default();
    installed(&ports, "app_pay", &[Scope::PaymentsWrite]);
    let listed = block_on(list_extension_endpoints(&Ctx::system(), &ports, "app_pay")).unwrap();
    assert_eq!(
        listed.iter().map(|e| e.url.as_str()).collect::<Vec<_>>(),
        ["https://pay.example/base/start", "https://pay.example/void"]
    );
    let input = EndpointInput {
        point: "payments.start".to_string(),
        version: 1,
        url: "https://elsewhere.example/start".to_string(),
    };
    assert_eq!(
        block_on(set_extension_endpoints(
            &Ctx::system(),
            &ports,
            "app_pay",
            vec![input],
            &[START],
        )),
        Err(CommandError::App(AppError::EndpointsFromManifest(key(
            "app_pay"
        ))))
    );
}
