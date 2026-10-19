//! App UI in any admin and storefront (design note 0029, §5–§8): discovery, session
//! tokens, actions, blocks, the app proxy and manifest refreshes, against a local app.

use axum::{
    Router,
    body::Bytes,
    http::{HeaderMap, Method as HttpMethod, StatusCode, Uri},
    routing::{any, get, post},
};
use base64::Engine;
use serde_json::{Value, json};

use super::oauth::{install, oauth_app};
use super::*;

/// What the app was sent: one entry per request, `{method, uri, headers, body}`.
type Seen = Arc<Mutex<Vec<Value>>>;

/// The app's manifest, which a test may change before a refresh.
type Manifest = Arc<Mutex<Value>>;

fn manifest(base: &str, scopes: &[&str]) -> Value {
    json!({
        "key": "app_ui",
        "name": "UI app",
        "version": "1.0.0",
        "app_url": base,
        "redirect_uris": ["/callback"],
        "scopes": scopes,
        "privacy_url": "/privacy",
        "settings": { "schema": { "type": "object" }, "secret_fields": ["api_key"] },
        "admin": {
            "pages": [
                { "id": "dashboard", "title": "Dashboard", "url": "/admin", "nav": "apps" },
                { "id": "settings", "title": "Settings", "url": "/settings", "nav": "settings" }
            ],
            "blocks": [
                { "id": "payment", "target": "order.payment", "url": "/blocks/payment" },
                { "id": "bad", "target": "order.details", "url": "/blocks/bad" },
                { "id": "who", "target": "customer.details", "url": "/blocks/who" },
                { "id": "later", "target": "order.timeline", "url": "/blocks/later" }
            ],
            "actions": [
                { "id": "capture", "target": "payment", "label": "Capture", "url": "/actions/capture",
                  "input_schema": { "type": "object" } },
                { "id": "refund", "target": "payment", "label": "Refund", "point": "payments.refund" }
            ],
            "proxy": "/admin-api"
        },
        "storefront": {
            "elements": [{ "id": "pay", "target": "checkout.payment", "module": "/el.js",
                           "tag": "ui-pay" }],
            "proxy": "/sf"
        }
    })
}

/// The request as the app saw it.
fn seen(method: &HttpMethod, uri: &Uri, headers: &HeaderMap, body: &[u8]) -> Value {
    let headers: serde_json::Map<String, Value> = headers
        .iter()
        .map(|(n, v)| (n.as_str().to_string(), json!(v.to_str().unwrap_or(""))))
        .collect();
    json!({
        "method": method.as_str(),
        "uri": uri.to_string(),
        "headers": headers,
        "body": String::from_utf8_lossy(body),
    })
}

/// A local app with every kind of UI contribution. Returns its base URL, its manifest
/// (to change) and what it was sent.
async fn ui_app(scopes: &[&str]) -> (String, Manifest, Seen) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let doc: Manifest = Arc::new(Mutex::new(manifest(&base, scopes)));
    let log: Seen = Arc::default();
    let record = |log: &Seen| {
        let log = Arc::clone(log);
        move |method: HttpMethod, uri: Uri, headers: HeaderMap, body: Bytes| {
            log.lock()
                .unwrap()
                .push(seen(&method, &uri, &headers, &body));
        }
    };
    let served = Arc::clone(&doc);
    let (block, bad, action, proxy) = (record(&log), record(&log), record(&log), record(&log));
    let echo = Arc::clone(&log);
    let router = Router::new()
        .route(
            "/hypha-app.json",
            get(move || {
                let doc = served.lock().unwrap().to_string();
                async move { ([(CONTENT_TYPE, "application/json")], doc) }
            }),
        )
        .route(
            "/blocks/payment",
            get(move |m: HttpMethod, u: Uri, h: HeaderMap, b: Bytes| {
                block(m, u, h, b);
                async {
                    let tree = json!({ "version": 1, "root": { "type": "card", "title": "Paid",
                        "children": [{ "type": "badge", "value": "captured", "tone": "success" },
                                     { "type": "sparkles" }] } });
                    // A 2xx other than 200: the call log keeps the status answered.
                    (
                        StatusCode::NON_AUTHORITATIVE_INFORMATION,
                        [(CONTENT_TYPE, "application/json")],
                        tree.to_string(),
                    )
                }
            }),
        )
        .route(
            "/blocks/bad",
            get(move |m: HttpMethod, u: Uri, h: HeaderMap, b: Bytes| {
                bad(m, u, h, b);
                async {
                    let tree = json!({ "version": 1, "root": { "type": "link", "label": "x",
                        "href": "javascript:alert(1)" } });
                    ([(CONTENT_TYPE, "application/json")], tree.to_string())
                }
            }),
        )
        .route(
            "/actions/capture",
            post(move |m: HttpMethod, u: Uri, h: HeaderMap, b: Bytes| {
                action(m, u, h, b);
                async {
                    let answer = json!({ "message": "Captured", "refresh": true });
                    ([(CONTENT_TYPE, "application/json")], answer.to_string())
                }
            }),
        )
        .route(
            "/admin-api/{*rest}",
            any(move |m: HttpMethod, u: Uri, h: HeaderMap, b: Bytes| {
                proxy(m.clone(), u.clone(), h.clone(), b.clone());
                async move {
                    let status = if m == HttpMethod::POST {
                        StatusCode::CREATED
                    } else {
                        StatusCode::OK
                    };
                    (
                        status,
                        [(CONTENT_TYPE, "application/json"), (SET_COOKIE, "s=1")],
                        seen(&m, &u, &h, &b).to_string(),
                    )
                }
            }),
        )
        .route(
            "/sf/{*rest}",
            any(move |m: HttpMethod, u: Uri, h: HeaderMap, b: Bytes| {
                echo.lock().unwrap().push(seen(&m, &u, &h, &b));
                async move {
                    (
                        [(CONTENT_TYPE, "text/plain; charset=utf-8")],
                        seen(&m, &u, &h, &b).to_string(),
                    )
                }
            }),
        );
    tokio::spawn(async move { axum::serve(listener, router).await });
    (base, doc, log)
}

use axum::http::header::SET_COOKIE;

/// The last request the app saw.
fn last(log: &Seen) -> Value {
    log.lock().unwrap().last().cloned().unwrap_or(Value::Null)
}

/// `headers`' `hypha-*` entries, as `context_signature` takes them.
fn hypha_headers(headers: &Value) -> Vec<(String, String)> {
    headers
        .as_object()
        .unwrap()
        .iter()
        .filter(|(n, _)| n.starts_with("hypha-"))
        .map(|(n, v)| (n.clone(), v.as_str().unwrap().to_string()))
        .collect()
}

/// Whether the request the app saw carries both signatures, by `secret`.
fn signed(request: &Value, secret: &str, method: hypha_http::outbound::Method) -> bool {
    let headers = &request["headers"];
    let body = headers["hypha-signature"].as_str().unwrap_or("");
    let at: i64 = body
        .split(',')
        .find_map(|p| p.strip_prefix("t="))
        .and_then(|t| t.parse().ok())
        .unwrap_or(0);
    let raw = request["body"].as_str().unwrap().as_bytes();
    body == hypha_http::outbound::sign(secret, at, raw)
        && headers["hypha-context-signature"].as_str()
            == Some(&hypha_http::outbound::context_signature(
                secret,
                at,
                method,
                &hypha_headers(headers),
            ))
}

/// The claims of a session token, after checking its signature under `secret`.
fn claims(token: &str, secret: &str) -> Value {
    let payload = token.split('.').nth(1).unwrap();
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .unwrap();
    let c: Value = serde_json::from_slice(&decoded).unwrap();
    let text = |k: &str| c[k].as_str().unwrap().to_string();
    let resigned = hypha_http::session_token::sign_session_token(
        &hypha_http::session_token::SessionClaims {
            iss: text("iss"),
            aud: text("aud"),
            sub: text("sub"),
            store: text("store"),
            scope: text("scope"),
            iat: c["iat"].as_i64().unwrap(),
            exp: c["exp"].as_i64().unwrap(),
            jti: text("jti"),
        },
        secret,
    );
    assert_eq!(resigned, token, "the token is signed with the app's secret");
    c
}

async fn me(app: &TestApp, session: &str) -> String {
    let (_, me) = send_as(app, session, "GET", "/admin/me", None).await;
    me["user"]["id"]
        .as_str()
        .or_else(|| me["id"].as_str())
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn admins_discover_open_run_and_fetch_what_an_app_adds() {
    let app = oauth_app().await;
    let (base, _, log) = ui_app(&["orders:read", "products:read"]).await;
    let tokens = install(&app, &base, None).await;
    let secret = tokens["app_secret"].as_str().unwrap().to_string();
    let owner = owner_session(&app).await;

    // Discovery: everything for the admin, but the customer block (no customers:read
    // in the grant) and the block on a target this Hypha doesn't know.
    let (status, all) = send_as(&app, &owner, "GET", "/admin/app-extensions", None).await;
    assert_eq!(status, StatusCode::OK, "{all}");
    assert_eq!(
        all["apps"],
        json!([{"key": "app_ui", "name": "UI app", "icon": null}])
    );
    assert_eq!(
        all["pages"],
        json!([
            {"app": "app_ui", "id": "dashboard", "title": "Dashboard", "url": format!("{base}/admin"),
             "nav": "apps", "target": "apps.page"},
            {"app": "app_ui", "id": "settings", "title": "Settings", "url": format!("{base}/settings"),
             "nav": "settings", "target": "settings.app"}
        ])
    );
    let blocks: Vec<&str> = all["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["id"].as_str().unwrap())
        .collect();
    assert_eq!(blocks, ["payment", "bad"]);
    assert_eq!(
        all["actions"],
        json!([
            {"app": "app_ui", "id": "capture", "label": "Capture", "target": "payment",
             "input_schema": {"type": "object"}, "point": null},
            {"app": "app_ui", "id": "refund", "label": "Refund", "target": "payment",
             "input_schema": null, "point": "payments.refund"}
        ])
    );
    assert_eq!(
        all["settings"],
        json!([{"app": "app_ui", "schema": {"type": "object"}, "secret_fields": ["api_key"],
                "proxy": true}])
    );
    let (status, payment) = send_as(
        &app,
        &owner,
        "GET",
        "/admin/app-extensions?target=order.payment",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(payment["blocks"].as_array().unwrap().len(), 1);
    assert_eq!(payment["pages"], json!([]));
    assert_eq!(payment["settings"], json!([]));
    let (status, body) = send_as(
        &app,
        &owner,
        "GET",
        "/admin/app-extensions?target=order.paymnet",
        None,
    )
    .await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::BAD_REQUEST, "invalid_query")
    );

    // A session token for the embedded page: the staff member, the app's scopes.
    let user = me(&app, &owner).await;
    let path = "/admin/app-extensions/app_ui/session";
    let (status, session) = send_as(&app, &owner, "POST", path, None).await;
    assert_eq!(status, StatusCode::OK, "{session}");
    let c = claims(session["token"].as_str().unwrap(), &secret);
    assert_eq!(c["aud"], "app_ui");
    assert_eq!(c["sub"], user.as_str());
    assert_eq!(c["store"], app.state.store.to_string());
    assert_eq!(c["scope"], "products:read orders:read");
    assert_eq!(c["iss"], "http://localhost");
    assert_eq!(c["exp"].as_i64().unwrap() - c["iat"].as_i64().unwrap(), 60);
    let (status, body) = send(&app, "POST", path, None).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::UNAUTHORIZED, "session_required")
    );
    let (status, body) = send_as(
        &app,
        &owner,
        "POST",
        "/admin/app-extensions/app_none/session",
        None,
    )
    .await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::NOT_FOUND, "app_not_found")
    );

    // An action: the app is called, signed, with the context, and answers.
    let order = format!("ord_{:032x}", 1);
    let pay = format!("pay_{:032x}", 2);
    let run = json!({"context": {"payment_id": pay, "order_id": order}, "input": {"note": "x"}})
        .to_string();
    let path = "/admin/app-extensions/actions/app_ui/capture";
    let (status, answer) = send_as(&app, &owner, "POST", path, Some(&run)).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(
        answer,
        json!({"message": "Captured", "refresh": true, "navigate": null})
    );
    let sent = last(&log);
    let body: Value = serde_json::from_str(sent["body"].as_str().unwrap()).unwrap();
    assert_eq!(
        body,
        json!({"action": "capture", "target": "payment",
               "context": {"order_id": order, "payment_id": pay},
               "input": {"note": "x"}, "staff": user, "scope": "products:read orders:read"})
    );
    assert_eq!(sent["headers"]["hypha-extension"], "admin.action");
    let (status, calls) = send(&app, "GET", "/admin/apps/app_ui/extension-calls", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(calls["items"][0]["point"], "admin.action");
    assert_eq!(calls["items"][0]["outcome"], "ok");
    assert_eq!(calls["items"][0]["correlation"], order.as_str());

    let missing = json!({"context": {"product_id": format!("prod_{:032x}", 3)}}).to_string();
    let (status, body) = send_as(&app, &owner, "POST", path, Some(&missing)).await;
    assert_eq!(
        (status, code(&body)),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_extension_context"
        )
    );
    let refund = "/admin/app-extensions/actions/app_ui/refund";
    let ctx = json!({"context": {"payment_id": pay}}).to_string();
    let (status, body) = send_as(&app, &owner, "POST", refund, Some(&ctx)).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::CONFLICT, "app_action_has_point")
    );
    let (status, body) = send_as(
        &app,
        &owner,
        "POST",
        "/admin/app-extensions/actions/app_ui/nope",
        Some(&ctx),
    )
    .await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::NOT_FOUND, "app_extension_not_found")
    );
    // A key that can't read orders can't use what's about them.
    let reader = key(
        &app,
        json!({"kind": "secret", "name": "a", "scopes": ["apps:read"]}),
    )
    .await;
    let (status, body) = send_as(&app, &reader, "POST", path, Some(&run)).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::FORBIDDEN, "missing_scope")
    );

    // A block: fetched signed, with the context, checked, returned.
    let uri =
        format!("/admin/app-extensions/blocks/app_ui/payment?order_id={order}&payment_id={pay}");
    let (status, block) = send_as(&app, &owner, "GET", &uri, None).await;
    assert_eq!(status, StatusCode::OK, "{block}");
    assert_eq!(block["target"], "order.payment");
    assert_eq!(block["tree"]["root"]["title"], "Paid");
    let sent = last(&log);
    assert_eq!(
        sent["uri"],
        format!("/blocks/payment?order_id={order}&payment_id={pay}")
    );
    assert_eq!(sent["headers"]["hypha-staff"], user.as_str());
    assert_eq!(sent["headers"]["hypha-target"], "order.payment");
    assert!(
        signed(&sent, &secret, hypha_http::outbound::Method::Get),
        "{sent}"
    );
    let bearer = sent["headers"]["authorization"].as_str().unwrap();
    assert_eq!(
        claims(bearer.strip_prefix("Bearer ").unwrap(), &secret)["sub"],
        user.as_str()
    );
    let (_, calls) = send(
        &app,
        "GET",
        "/admin/apps/app_ui/extension-calls?limit=1",
        None,
    )
    .await;
    let call = &calls["items"][0];
    assert_eq!(
        (&call["point"], &call["outcome"], &call["status"]),
        (&json!("admin.block"), &json!("ok"), &json!(203)),
        "{calls}"
    );
    let (status, body) = send_as(
        &app,
        &owner,
        "GET",
        "/admin/app-extensions/blocks/app_ui/payment",
        None,
    )
    .await;
    assert_eq!(
        (status, code(&body)),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_extension_context"
        )
    );
    let uri = format!("/admin/app-extensions/blocks/app_ui/bad?order_id={order}");
    let (status, body) = send_as(&app, &owner, "GET", &uri, None).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::BAD_GATEWAY, "app_unavailable")
    );
    assert_eq!(
        body["details"],
        json!({"app": "app_ui", "point": "admin.block"})
    );
    let (_, calls) = send(
        &app,
        "GET",
        "/admin/apps/app_ui/extension-calls?limit=1",
        None,
    )
    .await;
    assert_eq!(calls["items"][0]["outcome"], "invalid");
    assert!(
        calls["items"][0]["error"]
            .as_str()
            .unwrap()
            .contains("root.href"),
        "{calls}"
    );

    // The admin proxy: the method, path, query and body go to the app; its status,
    // content type and body come back, nothing else.
    let request = Request::builder()
        .method("POST")
        .uri("/admin/apps/app_ui/proxy/settings/save?x=1")
        .header(AUTHORIZATION, format!("Bearer {owner}"))
        .header(CONTENT_TYPE, "application/json")
        .header("cookie", "admin=secret")
        .body(Body::from(r#"{"a":1}"#))
        .unwrap();
    let response = app.router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(response.headers()[CONTENT_TYPE], "application/json");
    assert!(response.headers().get(SET_COOKIE).is_none());
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let echoed: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(echoed["uri"], "/admin-api/settings/save?x=1");
    assert_eq!(echoed["body"], r#"{"a":1}"#);
    assert_eq!(echoed["headers"]["hypha-path"], "/settings/save?x=1");
    assert_eq!(echoed["headers"]["hypha-staff"], user.as_str());
    assert!(echoed["headers"].get("cookie").is_none());
    assert!(
        signed(&echoed, &secret, hypha_http::outbound::Method::Post),
        "{echoed}"
    );
    let (status, body) = send_as(
        &app,
        &owner,
        "GET",
        "/admin/apps/app_ui/proxy/a/%2e%2e/b",
        None,
    )
    .await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::BAD_REQUEST, "invalid_proxy_path")
    );

    // The admin origins, for the app's frame-ancestors.
    let (status, origins) =
        send_as(&app, &owner, "GET", "/admin/app-extensions/origins", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(origins, json!({"origins": ["http://admin.test"]}));
}

#[tokio::test]
async fn storefronts_discover_elements_and_reach_the_app_with_the_shoppers_cart() {
    let app = oauth_app().await;
    let (base, _, log) = ui_app(&["orders:read"]).await;
    install(&app, &base, None).await;
    let _ = market_selling_a_mug(&app).await;
    let pk = publishable(&app).await;

    let (status, found) = send_as(&app, &pk, "GET", "/storefront/app-extensions", None).await;
    assert_eq!(status, StatusCode::OK, "{found}");
    assert_eq!(
        found,
        json!({
            "elements": [{"app": "app_ui", "id": "pay", "target": "checkout.payment",
                          "tag": "ui-pay", "module": format!("{base}/el.js")}],
            "proxies": [{"app": "app_ui"}]
        })
    );
    let (status, none) = send_as(
        &app,
        &pk,
        "GET",
        "/storefront/app-extensions?target=cart.block",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(none["elements"], json!([]));

    let (_, cart) = send_as(
        &app,
        &pk,
        "POST",
        "/storefront/carts",
        Some(r#"{"country":"HR"}"#),
    )
    .await;
    let cart = cart["id"].as_str().unwrap().to_string();
    let auth = format!("Bearer {pk}");
    let (status, _) = send_with(
        &app,
        &[(AUTHORIZATION.as_str(), &auth), ("x-hypha-cart", &cart)],
        "GET",
        "/storefront/apps/app_ui/proxy/reviews?page=2",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let sent = last(&log);
    assert_eq!(sent["uri"], "/sf/reviews?page=2");
    assert_eq!(sent["headers"]["hypha-cart"], cart.as_str());
    assert_eq!(sent["headers"]["hypha-country"], "HR");
    assert!(
        sent["headers"]["hypha-channel"]
            .as_str()
            .unwrap()
            .starts_with("chan_")
    );
    assert!(sent["headers"].get("hypha-staff").is_none());
    let (status, body) = send_with(
        &app,
        &[(AUTHORIZATION.as_str(), &auth), ("x-hypha-cart", "nope")],
        "GET",
        "/storefront/apps/app_ui/proxy/reviews",
        None,
    )
    .await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::BAD_REQUEST, "invalid_cart_header")
    );
    // An admin credential isn't a storefront's.
    let (status, body) = send(&app, "GET", "/storefront/apps/app_ui/proxy/reviews", None).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::FORBIDDEN, "publishable_key_required")
    );
}

#[tokio::test]
async fn a_refreshed_manifest_applies_and_new_scopes_wait_for_staff() {
    let app = oauth_app().await;
    let (base, doc, _) = ui_app(&["orders:read", "products:read"]).await;
    let tokens = install(&app, &base, None).await;
    let access = tokens["access_token"].as_str().unwrap().to_string();
    let owner = owner_session(&app).await;

    // Fewer scopes: at once. The app may refresh itself.
    *doc.lock().unwrap() = manifest(&base, &["orders:read"]);
    let (status, body) = send_as(&app, &access, "POST", "/admin/apps/app_ui/refresh", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "updated");
    assert_eq!(body["app"]["grant"]["scopes"], json!(["orders:read"]));

    // More: a request staff approve; the grant stays until then.
    *doc.lock().unwrap() = manifest(&base, &["orders:read", "customers:read"]);
    let (status, body) = send(&app, "POST", "/admin/apps/app_ui/refresh", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "consent_required");
    assert_eq!(body["app"]["grant"]["scopes"], json!(["orders:read"]));
    let request = body["request"].as_str().unwrap().to_string();
    assert_eq!(
        body["consent_url"],
        format!("http://admin.test/apps/consent?request={request}")
    );
    let path = format!("/admin/oauth/requests/{request}/approve");
    let (status, answer) = send_as(&app, &owner, "POST", &path, None).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(answer["redirect_to"], base.as_str());
    let (_, detail) = send(&app, "GET", "/admin/apps/app_ui", None).await;
    assert_eq!(
        detail["grant"]["scopes"],
        json!(["customers:read", "orders:read"])
    );

    // A broken manifest is refused; an unknown app is 404.
    *doc.lock().unwrap() = json!({"key": "app_ui"});
    let (status, body) = send(&app, "POST", "/admin/apps/app_ui/refresh", None).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::UNPROCESSABLE_ENTITY, "manifest_invalid")
    );
    let (status, body) = send(&app, "POST", "/admin/apps/app_none/refresh", None).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::NOT_FOUND, "app_not_found")
    );
}
