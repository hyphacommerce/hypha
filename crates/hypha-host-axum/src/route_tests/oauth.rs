//! Apps installed through OAuth and personal data: the whole dance over HTTP, and the
//! redaction contract on every admin route that returns customers, orders, carts or emails.

use axum::{
    Router,
    body::{Body, Bytes},
    http::{HeaderMap, Request, StatusCode, header::LOCATION},
    routing::{get, post},
};
use serde_json::{Value, json};
use tower::ServiceExt;

use super::*;

const VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
const CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";

/// What the app's webhook received: the envelopes, oldest first, each with the
/// delivery's `Hypha-Store` added as `_store`.
type Received = Arc<Mutex<Vec<Value>>>;

/// A local app: serves its manifest at `/hypha-app.json` (asking for `scopes`) and
/// takes webhooks at `/hooks`. Returns its base URL and what it received.
async fn local_app(scopes: &[&str]) -> (String, Received) {
    local_app_with_events(scopes, &["order.*"]).await
}

/// `local_app`, its webhook subscribed to `events`.
async fn local_app_with_events(scopes: &[&str], events: &[&str]) -> (String, Received) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let reasons = if scopes.contains(&"customer_data:email") {
        json!({ "customer_data:email": "to send receipts" })
    } else {
        json!({})
    };
    let manifest = json!({
        "key": "app_helper",
        "name": "Helper",
        "version": "1.0.0",
        "publisher": "Helpers Ltd",
        "app_url": base,
        "redirect_uris": ["/callback"],
        "scopes": scopes,
        "scope_reasons": reasons,
        "privacy_url": "/privacy",
        "webhooks": { "url": "/hooks", "events": events }
    })
    .to_string();
    let received: Received = Arc::default();
    let log = Arc::clone(&received);
    let router = Router::new()
        .route(
            "/hypha-app.json",
            get(move || {
                let manifest = manifest.clone();
                async move { ([(CONTENT_TYPE, "application/json")], manifest) }
            }),
        )
        .route(
            "/hooks",
            post(move |headers: HeaderMap, body: Bytes| {
                let log = Arc::clone(&log);
                async move {
                    let mut envelope: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
                    if let Some(fields) = envelope.as_object_mut() {
                        let store = headers.get("hypha-store").and_then(|v| v.to_str().ok());
                        fields.insert("_store".to_string(), json!(store));
                    }
                    log.lock().unwrap().push(envelope);
                    StatusCode::NO_CONTENT
                }
            }),
        );
    tokio::spawn(async move { axum::serve(listener, router).await });
    (base, received)
}

pub(super) async fn oauth_app() -> TestApp {
    app_configured("[server]\nadmin_url = \"http://admin.test\"").await
}

/// A request with no credentials; the status, the `Location` and the JSON body.
pub(super) async fn raw(
    app: &TestApp,
    method: &str,
    uri: &str,
    form: Option<String>,
) -> (StatusCode, Option<String>, Value) {
    let mut request = Request::builder().method(method).uri(uri);
    if form.is_some() {
        request = request.header(CONTENT_TYPE, "application/x-www-form-urlencoded");
    }
    let request = request
        .body(form.map_or_else(Body::empty, Body::from))
        .unwrap();
    let response = app.router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let location = response
        .headers()
        .get(LOCATION)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (
        status,
        location,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

pub(super) fn query_param(url: &str, name: &str) -> Option<String> {
    url::Url::parse(url)
        .unwrap()
        .query_pairs()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.into_owned())
}

pub(super) fn encode(pairs: &[(&str, &str)]) -> String {
    url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(pairs)
        .finish()
}

fn authorize_uri(base: &str, scope: Option<&str>) -> String {
    let mut pairs = vec![
        ("response_type", "code".to_string()),
        ("client_id", format!("{base}/hypha-app.json")),
        ("redirect_uri", format!("{base}/callback")),
        ("state", "st4te".to_string()),
        ("code_challenge", CHALLENGE.to_string()),
        ("code_challenge_method", "S256".to_string()),
    ];
    if let Some(scope) = scope {
        pairs.push(("scope", scope.to_string()));
    }
    let pairs: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (*k, v.as_str())).collect();
    format!("/oauth/authorize?{}", encode(&pairs))
}

/// `/oauth/authorize`, the consent (approved by the owner) and the code exchange:
/// the token response.
pub(super) async fn install(app: &TestApp, base: &str, scope: Option<&str>) -> Value {
    let (status, location, body) = raw(app, "GET", &authorize_uri(base, scope), None).await;
    assert_eq!(status, StatusCode::FOUND, "{body}");
    let location = location.unwrap();
    let code = match query_param(&location, "request") {
        Some(request) => {
            assert!(location.starts_with("http://admin.test/apps/consent?request=oareq_"));
            let owner = owner_session(app).await;
            let path = format!("/admin/oauth/requests/{request}/approve");
            let (status, answer) = send_as(app, &owner, "POST", &path, None).await;
            assert_eq!(status, StatusCode::OK, "{answer}");
            let to = answer["redirect_to"].as_str().unwrap();
            assert_eq!(query_param(to, "state").as_deref(), Some("st4te"));
            query_param(to, "code").unwrap()
        }
        // Nothing new to approve: straight back to the app.
        None => query_param(&location, "code").unwrap(),
    };
    let client = format!("{base}/hypha-app.json");
    let redirect = format!("{base}/callback");
    let form = encode(&[
        ("grant_type", "authorization_code"),
        ("code", &code),
        ("code_verifier", VERIFIER),
        ("client_id", &client),
        ("redirect_uri", &redirect),
    ]);
    let (status, _, tokens) = raw(app, "POST", "/oauth/token", Some(form)).await;
    assert_eq!(status, StatusCode::OK, "{tokens}");
    tokens
}

#[tokio::test]
async fn an_installed_app_gets_custom_field_definition_events_with_custom_fields_read() {
    let app = oauth_app().await;
    let (base, received) =
        local_app_with_events(&["custom_fields:read"], &["custom_field_definition.*"]).await;
    install(&app, &base, None).await;
    let body = json!({"owner_type": "product", "key": "material", "type": "text",
                      "name": "Material"})
    .to_string();
    let (status, created) =
        send(&app, "POST", "/admin/custom-field-definitions", Some(&body)).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    hypha_http::jobs::dispatch(&app.state).await.unwrap();
    let types: Vec<Value> = received
        .lock()
        .unwrap()
        .iter()
        .map(|e| e["type"].clone())
        .filter(|t| t != "app.installed")
        .collect();
    assert_eq!(types, [json!("custom_field_definition.created")]);
}

#[tokio::test]
async fn an_app_is_installed_acts_with_its_grant_and_is_uninstalled() {
    let app = oauth_app().await;
    let (base, received) =
        local_app(&["orders:read", "customers:read", "customer_data:email"]).await;

    // The consent page's view of the request.
    let (status, location, body) = raw(&app, "GET", &authorize_uri(&base, None), None).await;
    assert_eq!(status, StatusCode::FOUND, "{body}");
    let request = query_param(&location.unwrap(), "request").unwrap();
    let owner = owner_session(&app).await;
    let path = format!("/admin/oauth/requests/{request}");
    let (status, consent) = send_as(&app, &owner, "GET", &path, None).await;
    assert_eq!(status, StatusCode::OK, "{consent}");
    assert_eq!(consent["status"], "pending");
    assert_eq!(consent["app"]["name"], "Helper");
    assert_eq!(consent["app"]["publisher"], "Helpers Ltd");
    assert_eq!(consent["app"]["privacy_url"], format!("{base}/privacy"));
    assert_eq!(consent["app"]["installed"], false);
    assert_eq!(
        consent["groups"],
        json!([{"area": "customers", "scopes": ["customers:read"]},
               {"area": "orders", "scopes": ["orders:read"]}])
    );
    assert_eq!(
        consent["personal_data"],
        json!([{"scope": "customer_data:email", "reason": "to send receipts"}])
    );
    assert_eq!(consent["changes"]["removed"], json!([]));

    // Approving needs every scope asked for: a key without them is refused.
    let manager = key(
        &app,
        json!({"kind": "secret", "name": "m", "scopes": ["apps:write"]}),
    )
    .await;
    let (status, body) = send_as(&app, &manager, "POST", &format!("{path}/approve"), None).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::FORBIDDEN, "not_allowed_to_grant")
    );
    let (status, answer) = send_as(&app, &owner, "POST", &format!("{path}/approve"), None).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    let to = answer["redirect_to"].as_str().unwrap().to_string();
    assert!(to.starts_with(&format!("{base}/callback?")), "{to}");
    let code_value = query_param(&to, "code").unwrap();
    let (status, body) = send_as(&app, &owner, "POST", &format!("{path}/approve"), None).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::CONFLICT, "oauth_request_decided")
    );
    // Approved but without its secret yet: the app hears nothing until it exchanges.
    hypha_http::jobs::dispatch(&app.state).await.unwrap();
    assert_eq!(*received.lock().unwrap(), Vec::<Value>::new());

    // The code exchange: a wrong verifier first, then the right one.
    let client = format!("{base}/hypha-app.json");
    let redirect = format!("{base}/callback");
    let exchange = |verifier: &str| {
        encode(&[
            ("grant_type", "authorization_code"),
            ("code", &code_value),
            ("code_verifier", verifier),
            ("client_id", &client),
            ("redirect_uri", &redirect),
        ])
    };
    let wrong = format!("{}A", &VERIFIER[..VERIFIER.len() - 1]);
    let (status, _, body) = raw(&app, "POST", "/oauth/token", Some(exchange(&wrong))).await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::BAD_REQUEST, Some("invalid_grant"))
    );
    let (status, _, tokens) = raw(&app, "POST", "/oauth/token", Some(exchange(VERIFIER))).await;
    assert_eq!(status, StatusCode::OK, "{tokens}");
    assert_eq!(tokens["token_type"], "Bearer");
    assert_eq!(tokens["expires_in"], 3600);
    assert_eq!(
        tokens["scope"],
        "customers:read orders:read customer_data:email"
    );
    assert!(
        tokens["app_secret"]
            .as_str()
            .unwrap()
            .starts_with("hypha_as_")
    );
    let store = app.state.store.to_string();
    assert_eq!(tokens["store"], store);
    assert_eq!(tokens["issuer"], "http://localhost");
    let access = tokens["access_token"].as_str().unwrap().to_string();
    assert!(access.starts_with("hypha_at_"));
    // The app hears it's installed, now that it can verify the delivery.
    hypha_http::jobs::dispatch(&app.state).await.unwrap();
    {
        let received = received.lock().unwrap();
        let installed = received.first().expect("app.installed");
        assert_eq!(installed["type"], "app.installed");
        assert_eq!(installed["_store"], store);
        assert!(
            installed["subject"]["id"]
                .as_str()
                .unwrap()
                .starts_with("appid_")
        );
    }
    let refresh = tokens["refresh_token"].as_str().unwrap().to_string();

    // The token acts as the app, with its grant.
    let (status, body) = send_as(&app, &access, "GET", "/admin/orders", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = send_as(&app, &access, "GET", "/admin/products", None).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::FORBIDDEN, "app_not_authorized")
    );
    let (status, body) = send_as(&app, &access, "GET", "/admin/me", None).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::UNAUTHORIZED, "session_required")
    );
    let (status, detail) = send(&app, "GET", "/admin/apps/app_helper", None).await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert_eq!(detail["installed"], true);
    assert_eq!(
        detail["grant"]["scopes"],
        json!(["customers:read", "orders:read", "customer_data:email"])
    );
    assert_eq!(detail["grant"]["approved_by"]["type"], "user");
    assert_eq!(detail["manifest"]["url"], client);
    assert_eq!(
        detail["registrations"]["webhooks"]["url"],
        format!("{base}/hooks")
    );
    assert_eq!(detail["history"][0]["status"], "approved");

    // Refreshing rotates; the old refresh token used again revokes everything.
    let refreshed = encode(&[("grant_type", "refresh_token"), ("refresh_token", &refresh)]);
    let (status, _, second) = raw(&app, "POST", "/oauth/token", Some(refreshed.clone())).await;
    assert_eq!(status, StatusCode::OK, "{second}");
    assert!(second.get("app_secret").is_none());
    assert_eq!(
        (&second["store"], &second["issuer"]),
        (&json!(store), &json!("http://localhost"))
    );
    let second_access = second["access_token"].as_str().unwrap().to_string();
    let (status, _, body) = raw(&app, "POST", "/oauth/token", Some(refreshed)).await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::BAD_REQUEST, Some("invalid_grant"))
    );
    let (status, body) = send_as(&app, &second_access, "GET", "/admin/orders", None).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::UNAUTHORIZED, "invalid_token")
    );

    // Authorizing again with nothing new: straight back with a code, no consent.
    let third = install(&app, &base, None).await;
    assert!(
        third.get("app_secret").is_none(),
        "only the first install shows it"
    );
    let third_access = third["access_token"].as_str().unwrap().to_string();
    let form = encode(&[("token", &third_access)]);
    let (status, _, _) = raw(&app, "POST", "/oauth/revoke", Some(form)).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send_as(&app, &third_access, "GET", "/admin/orders", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Narrowing applies at once; widening is refused.
    let fourth = install(&app, &base, None).await;
    let fourth_access = fourth["access_token"].as_str().unwrap().to_string();
    let narrow = json!({"scopes": ["customers:read"]}).to_string();
    let (status, body) = send(&app, "PATCH", "/admin/apps/app_helper/grant", Some(&narrow)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    hypha_http::jobs::dispatch(&app.state).await.unwrap();
    let (status, body) = send_as(&app, &fourth_access, "GET", "/admin/orders", None).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::FORBIDDEN, "app_not_authorized")
    );
    let widen = json!({"scopes": ["customers:read", "products:read"]}).to_string();
    let (status, body) = send(&app, "PATCH", "/admin/apps/app_helper/grant", Some(&widen)).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::UNPROCESSABLE_ENTITY, "grant_not_narrower")
    );

    // Uninstalling: tokens stop, the app hears it last, a second DELETE removes it.
    let (status, _) = send(&app, "DELETE", "/admin/apps/app_helper", None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = send_as(&app, &fourth_access, "GET", "/admin/customers", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    hypha_http::jobs::dispatch(&app.state).await.unwrap();
    let types: Vec<String> = received
        .lock()
        .unwrap()
        .iter()
        .map(|e| e["type"].as_str().unwrap_or_default().to_string())
        .collect();
    assert_eq!(types.first().map(String::as_str), Some("app.installed"));
    assert_eq!(types.last().map(String::as_str), Some("app.uninstalled"));
    assert!(
        types.contains(&"app.grant_changed".to_string()),
        "{types:?}"
    );
    let (status, detail) = send(&app, "GET", "/admin/apps/app_helper", None).await;
    assert_eq!(
        (status, &detail["installed"]),
        (StatusCode::OK, &json!(false))
    );
    assert!(detail["uninstalled_at"].is_string());
    let (status, _) = send(&app, "DELETE", "/admin/apps/app_helper", None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = send(&app, "GET", "/admin/apps/app_helper", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn untrusted_clients_get_a_page_and_other_errors_go_back_to_the_app() {
    let app = oauth_app().await;
    let (base, _) = local_app(&["orders:read"]).await;
    let bad_redirect = authorize_uri(&base, None).replace("%2Fcallback", "%2Fother");
    let (status, location, body) = raw(&app, "GET", &bad_redirect, None).await;
    assert_eq!(
        (status, location, code(&body)),
        (StatusCode::BAD_REQUEST, None, "invalid_redirect_uri")
    );
    let unknown = authorize_uri(&base, None).replace("hypha-app.json", "nothing.json");
    let (status, _, body) = raw(&app, "GET", &unknown, None).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::BAD_REQUEST, "manifest_unreachable")
    );
    let (status, location, _) = raw(
        &app,
        "GET",
        &authorize_uri(&base, Some("products:write")),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FOUND);
    let location = location.unwrap();
    assert!(location.starts_with(&format!("{base}/callback?")));
    assert_eq!(
        query_param(&location, "error").as_deref(),
        Some("invalid_scope")
    );
    assert_eq!(query_param(&location, "state").as_deref(), Some("st4te"));

    let (status, _, body) = raw(
        &app,
        "POST",
        "/oauth/token",
        Some("grant_type=password".into()),
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::BAD_REQUEST, Some("unsupported_grant_type"))
    );
    let (status, _, metadata) =
        raw(&app, "GET", "/.well-known/oauth-authorization-server", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        metadata["code_challenge_methods_supported"],
        json!(["S256"])
    );
    assert!(
        metadata["token_endpoint"]
            .as_str()
            .unwrap()
            .ends_with("/oauth/token")
    );
}

#[tokio::test]
async fn without_an_admin_url_there_is_no_consent_page() {
    let app = app().await;
    let (base, _) = local_app(&["orders:read"]).await;
    let (status, _, body) = raw(&app, "GET", &authorize_uri(&base, None), None).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::SERVICE_UNAVAILABLE, "admin_url_not_configured")
    );
}

// ---------- personal data ----------

/// Values that must never reach a caller without the `customer_data:*` scopes.
const PII: [&str; 10] = [
    "pii-ana@example.hr",
    "Piiana",
    "Piihorvat",
    "+385 PII 1",
    "Piistreet 1",
    "Piiflat 2",
    "Piicity",
    "PII-10000",
    "Piiprovince",
    // A personal custom field's value.
    "Piinote",
];

fn leaks(body: &Value) -> Vec<&'static str> {
    let text = body.to_string();
    PII.into_iter().filter(|p| text.contains(p)).collect()
}

/// A customer with every personal field set, an order placed with their email, a cart
/// with their shipping address linked to them; their IDs.
async fn personal_data(app: &TestApp) -> (String, String, String) {
    let (order, _, _) = placed_order_for_three_mugs(app).await;
    let address = json!({"first_name": "Piiana", "last_name": "Piihorvat", "company": "Acme",
        "line1": "Piistreet 1", "line2": "Piiflat 2", "city": "Piicity",
        "province": "Piiprovince", "postal_code": "PII-10000", "country": "HR",
        "phone": "+385 PII 1"});
    let body = json!({"email": "pii-ana@example.hr", "first_name": "Piiana",
        "last_name": "Piihorvat", "phone": "+385 PII 1"})
    .to_string();
    let (status, customer) = send(app, "POST", "/admin/customers", Some(&body)).await;
    assert_eq!(status, StatusCode::CREATED, "{customer}");
    let customer = customer["id"].as_str().unwrap().to_string();
    let (status, body) = send(
        app,
        "POST",
        &format!("/admin/customers/{customer}/addresses"),
        Some(&address.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let pk = publishable(app).await;
    let (_, cart) = send_as(
        app,
        &pk,
        "POST",
        "/storefront/carts",
        Some(r#"{"country":"HR"}"#),
    )
    .await;
    let cart = cart["id"].as_str().unwrap().to_string();
    let cart_path = format!("/storefront/carts/{cart}");
    let email = json!({"email": "pii-ana@example.hr"}).to_string();
    send_as(app, &pk, "PUT", &format!("{cart_path}/email"), Some(&email)).await;
    let ship = json!({"address": address}).to_string();
    let (status, body) = send_as(
        app,
        &pk,
        "PUT",
        &format!("{cart_path}/shipping-address"),
        Some(&ship),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    // The shopper sees their own data on the storefront.
    assert!(!leaks(&body).is_empty());
    (order, customer, cart)
}

#[tokio::test]
async fn no_admin_route_shows_personal_data_without_its_scopes() {
    let app = oauth_app().await;
    let (order, customer, cart) = personal_data(&app).await;
    // The order's email is the checkout's; make it a known one too.
    let (_, placed) = send(&app, "GET", &format!("/admin/orders/{order}"), None).await;
    let order_email = placed["email"].as_str().unwrap().to_string();
    // Personal custom fields on the customer and the order.
    let mut definitions = Vec::new();
    for (owner, path) in [
        (
            "customer",
            format!("/admin/customers/{customer}/custom-fields"),
        ),
        ("order", format!("/admin/orders/{order}/custom-fields")),
    ] {
        let body = json!({"owner_type": owner, "key": "note", "type": "text", "name": "Note",
            "personal": true, "capabilities": {"filterable": true}});
        let (status, created) = send(
            &app,
            "POST",
            "/admin/custom-field-definitions",
            Some(&body.to_string()),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{created}");
        definitions.push(created["id"].as_str().unwrap().to_string());
        let body = json!({"custom_fields": {"custom.note": "Piinote"}}).to_string();
        let (status, body) = send(&app, "PATCH", &path, Some(&body)).await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }

    // An app granted the records but no personal data, and a secret key alike.
    let (base, _) = local_app(&[
        "orders:write",
        "customers:write",
        "carts:write",
        "notifications:read",
        "apps:read",
        "customer_data:email",
    ])
    .await;
    let tokens = install(
        &app,
        &base,
        Some("orders:write customers:write carts:write notifications:read apps:read"),
    )
    .await;
    let app_token = tokens["access_token"].as_str().unwrap().to_string();
    let secret = key(
        &app,
        json!({"kind": "secret", "name": "dashboard", "scopes":
            ["orders:write", "customers:write", "carts:write", "notifications:read", "apps:read"]}),
    )
    .await;

    let link = json!({"customer": customer}).to_string();
    let routes: Vec<(&str, String, Option<String>)> = vec![
        ("GET", "/admin/customers".to_string(), None),
        ("GET", format!("/admin/customers/{customer}"), None),
        (
            "PATCH",
            format!("/admin/customers/{customer}"),
            Some(r#"{"phone": "+385 PII 1"}"#.to_string()),
        ),
        ("GET", "/admin/orders".to_string(), None),
        ("GET", format!("/admin/orders/{order}"), None),
        ("POST", format!("/admin/orders/{order}/paid"), None),
        ("PUT", format!("/admin/carts/{cart}/customer"), Some(link)),
        ("GET", format!("/admin/notifications?order={order}"), None),
        (
            "GET",
            "/admin/customers?custom_fields=custom.note".to_string(),
            None,
        ),
        (
            "GET",
            format!("/admin/customers/{customer}/custom-fields"),
            None,
        ),
        (
            "GET",
            "/admin/orders?custom_fields=custom.note".to_string(),
            None,
        ),
        ("GET", format!("/admin/orders/{order}/custom-fields"), None),
    ];
    // The export carries everything, so it needs every `customer_data:*` scope rather than
    // redaction: refused outright here.
    for token in [&app_token, &secret] {
        let (status, json) = send_as(
            &app,
            token,
            "GET",
            &format!("/admin/customers/{customer}/export"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{json}");
        assert!(
            !json.to_string().contains(&order_email),
            "the order's email in {json}"
        );
    }
    for token in [&app_token, &secret] {
        for (method, path, body) in &routes {
            let (status, json) = send_as(&app, token, method, path, body.as_deref()).await;
            assert!(status.is_success(), "{method} {path}: {status} {json}");
            assert_eq!(leaks(&json), Vec::<&str>::new(), "{method} {path}: {json}");
            assert!(
                !json.to_string().contains(&order_email),
                "{method} {path}: the order's email in {json}"
            );
        }
        // What was removed is listed.
        let (_, one) = send_as(
            &app,
            token,
            "GET",
            &format!("/admin/customers/{customer}"),
            None,
        )
        .await;
        assert_eq!(
            one["redacted"],
            json!([
                "first_name",
                "last_name",
                "email",
                "phone",
                "custom_fields.custom.note"
            ])
        );
        assert!(one["addresses"][0]["redacted"].is_array());
        assert_eq!(one["addresses"][0]["country"], "HR");
        assert_eq!(one["addresses"][0]["company"], "Acme");
        // Searching by a personal field would tell what it holds.
        let (status, body) = send_as(&app, token, "GET", "/admin/customers?q=pii", None).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        let (status, body) = send_as(&app, token, "GET", "/admin/orders?q=ana", None).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        let (status, _) = send_as(&app, token, "GET", "/admin/orders?q=1001", None).await;
        assert_eq!(status, StatusCode::OK);
        // So would filtering by a personal custom field, or listing its values.
        let (status, body) = send_as(
            &app,
            token,
            "GET",
            "/admin/customers?field[custom.note]=Piinote",
            None,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        for id in &definitions {
            let path = format!("/admin/custom-field-definitions/{id}/values-in-use");
            let (status, body) = send_as(&app, token, "GET", &path, None).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{path}: {body}");
        }
    }

    // With the scope, the field comes back (and only it).
    let full = key(
        &app,
        json!({"kind": "secret", "name": "crm", "scopes": ["customers:read", "customer_data:email"]}),
    )
    .await;
    let (_, one) = send_as(
        &app,
        &full,
        "GET",
        &format!("/admin/customers/{customer}"),
        None,
    )
    .await;
    assert_eq!(one["email"], "pii-ana@example.hr");
    assert_eq!(
        one["redacted"],
        json!([
            "first_name",
            "last_name",
            "phone",
            "custom_fields.custom.note"
        ])
    );
    // Staff see everything.
    let owner = owner_session(&app).await;
    let (_, one) = send_as(
        &app,
        &owner,
        "GET",
        &format!("/admin/customers/{customer}"),
        None,
    )
    .await;
    assert_eq!(one["first_name"], "Piiana");
    assert_eq!(one["custom_fields"]["custom.note"], "Piinote");
    assert!(one.get("redacted").is_none());
}

#[tokio::test]
async fn a_private_apps_manifest_is_uploaded_and_its_approval_shows_its_credentials_once() {
    let app = oauth_app().await;
    let owner = owner_session(&app).await;
    let manifest = json!({
        "key": "app_rates",
        "name": "Our rates",
        "version": "1.0.0",
        "app_url": "https://rates.internal.example",
        "redirect_uris": [],
        "scopes": ["shipping:read", "customer_data:address"],
        "scope_reasons": { "customer_data:address": "quotes by postal code" },
        "privacy_url": "/privacy"
    });

    // An API key can't install apps, even with apps:write.
    let writer = key(
        &app,
        json!({"kind": "secret", "name": "w", "scopes": ["apps:write"]}),
    )
    .await;
    let body = json!({ "manifest": manifest });
    let (status, answer) = send_as(
        &app,
        &writer,
        "POST",
        "/admin/apps/private",
        Some(&body.to_string()),
    )
    .await;
    assert_eq!(
        (status, code(&answer)),
        (StatusCode::UNAUTHORIZED, "session_required")
    );

    let mut bad = manifest.clone();
    bad["scope_reasons"] = json!({});
    let (status, answer) = send_as(
        &app,
        &owner,
        "POST",
        "/admin/apps/private",
        Some(&json!({ "manifest": bad }).to_string()),
    )
    .await;
    assert_eq!(
        (status, code(&answer)),
        (StatusCode::UNPROCESSABLE_ENTITY, "manifest_invalid")
    );
    assert_eq!(answer["details"]["errors"][0]["path"], "scope_reasons");

    let (status, created) = send_as(
        &app,
        &owner,
        "POST",
        "/admin/apps/private",
        Some(&body.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let request = created["request"]["id"].as_str().unwrap().to_string();
    assert_eq!(
        created["consent_url"],
        format!("http://admin.test/apps/consent?request={request}")
    );
    assert_eq!(created["request"]["app"]["private"], true);
    assert_eq!(
        created["request"]["app"]["client_id"],
        "urn:hypha:private:app_rates"
    );
    assert_eq!(
        created["request"]["personal_data"],
        json!([{"scope": "customer_data:address", "reason": "quotes by postal code"}])
    );

    let path = format!("/admin/oauth/requests/{request}/approve");
    let (status, answer) = send_as(&app, &owner, "POST", &path, None).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(answer["redirect_to"], Value::Null);
    let credentials = &answer["credentials"];
    assert_eq!(credentials["app"], "app_rates");
    assert_eq!(
        credentials["scopes"],
        json!(["shipping:read", "customer_data:address"])
    );
    assert!(
        credentials["token_endpoint"]
            .as_str()
            .unwrap()
            .ends_with("/oauth/token")
    );
    assert!(
        credentials["app_secret"]
            .as_str()
            .unwrap()
            .starts_with("hypha_as_")
    );
    let access = credentials["access_token"].as_str().unwrap().to_string();
    let (status, body) = send_as(&app, &access, "GET", "/admin/shipping-zones", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // Installed under its URN: nothing to refresh from.
    let (status, body) = send_as(&app, &owner, "POST", "/admin/apps/app_rates/refresh", None).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::CONFLICT, "app_is_private")
    );
    let (status, body) = send_as(&app, &owner, "GET", "/admin/apps/app_rates", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["installed"], true);
}
