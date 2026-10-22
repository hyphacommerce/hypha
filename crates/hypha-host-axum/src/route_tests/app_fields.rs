//! Apps' custom fields: created when an install is approved, following refreshes (a dropped
//! field waits for consent and goes with its values on approval), never retyped, kept on
//! uninstall and removed with the app.

use axum::{Router, routing::get};
use hypha_core::{AppKey, Ctx, ProductId};
use hypha_custom_fields::{
    CallerKind, DefinitionRepo, FieldCaller, HasCustomFieldDefinitions, Namespace, Owner,
    SetCustomFields, get_custom_fields, set_custom_fields,
};
use serde_json::{Value, json};

use super::oauth::{install, oauth_app, raw};
use super::*;

const CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";

fn erp_id(field_type: &str) -> Value {
    json!({ "owner_type": "product", "key": "erp_id", "type": field_type, "name": "ERP ID",
            "capabilities": { "unique": true, "filterable": true } })
}

fn notes() -> Value {
    json!({ "owner_type": "product", "key": "notes", "type": "multiline_text", "name": "Notes" })
}

fn color() -> Value {
    json!({ "owner_type": "product", "key": "color", "type": "text", "name": "Colour",
            "access": { "admin": "read_write", "storefront": "read" } })
}

fn manifest(base: &str, fields: Value) -> Value {
    json!({
        "key": "app_erp",
        "name": "ERP",
        "version": "1.0.0",
        "app_url": base,
        "redirect_uris": ["/callback"],
        "scopes": ["products:read"],
        "privacy_url": "/privacy",
        "custom_fields": fields
    })
}

/// A local app serving whatever manifest `doc` holds at `/hypha-app.json`.
async fn erp_app(fields: Value) -> (String, Arc<Mutex<Value>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let doc = Arc::new(Mutex::new(manifest(&base, fields)));
    let served = Arc::clone(&doc);
    let router = Router::new().route(
        "/hypha-app.json",
        get(move || {
            let body = served.lock().unwrap().to_string();
            async move { ([(CONTENT_TYPE, "application/json")], body) }
        }),
    );
    tokio::spawn(async move { axum::serve(listener, router).await });
    (base, doc)
}

/// The keys of `app_erp`'s definitions, as stored.
async fn stored(app: &TestApp) -> Vec<String> {
    let ports = app.state.public_ports().await.unwrap();
    let namespace = Namespace::of_app(AppKey::new("app_erp").unwrap());
    let mut keys: Vec<String> = ports
        .custom_field_definitions()
        .list_namespace(&namespace)
        .await
        .unwrap()
        .iter()
        .map(|d| d.key().to_string())
        .collect();
    keys.sort();
    keys
}

/// Writes `values` on `product` as the app would.
async fn write_as_app(app: &TestApp, product: &str, values: Value) {
    let ports = app.state.public_ports().await.unwrap();
    let kind = CallerKind::App(AppKey::new("app_erp").unwrap());
    let ctx = Ctx::system();
    let owner = Owner::product(product.parse::<ProductId>().unwrap());
    let input = SetCustomFields {
        values: serde_json::from_value(values).unwrap(),
        if_versions: Default::default(),
    };
    set_custom_fields(&FieldCaller::new(&ctx, &kind), &ports, &owner, input)
        .await
        .unwrap();
}

/// The keys `product` holds values for.
async fn values_of(app: &TestApp, product: &str) -> Vec<String> {
    let ports = app.state.public_ports().await.unwrap();
    let ctx = Ctx::system();
    let kind = CallerKind::Merchant;
    let owner = Owner::product(product.parse::<ProductId>().unwrap());
    let fields = get_custom_fields(&FieldCaller::new(&ctx, &kind), &ports, &owner)
        .await
        .unwrap();
    let mut keys: Vec<String> = fields.entries.iter().map(|e| e.key().to_string()).collect();
    keys.sort();
    keys
}

fn names(list: &Value) -> Vec<&str> {
    list.as_array()
        .unwrap()
        .iter()
        .map(|f| f["key"].as_str().unwrap())
        .collect()
}

#[tokio::test]
async fn an_apps_fields_are_created_on_install_and_follow_its_refreshes() {
    let app = oauth_app().await;
    let owner = owner_session(&app).await;
    let (base, doc) = erp_app(json!([erp_id("text"), notes()])).await;
    install(&app, &base, None).await;
    assert_eq!(stored(&app).await, ["erp_id", "notes"]);
    let product = create_product(&app, "erp-synced").await;
    write_as_app(
        &app,
        &product,
        json!({"app_erp.erp_id": "E-1", "app_erp.notes": "fragile"}),
    )
    .await;

    // A new field, no new scope: applied at once.
    *doc.lock().unwrap() = manifest(&base, json!([erp_id("text"), notes(), color()]));
    let (status, body) = send(&app, "POST", "/admin/apps/app_erp/refresh", None).await;
    assert_eq!(
        (status, &body["status"]),
        (StatusCode::OK, &json!("updated")),
        "{body}"
    );
    assert_eq!(stored(&app).await, ["color", "erp_id", "notes"]);

    // A dropped field would take its values: it waits for staff, who see it coming.
    *doc.lock().unwrap() = manifest(&base, json!([erp_id("text"), color()]));
    let (status, body) = send(&app, "POST", "/admin/apps/app_erp/refresh", None).await;
    assert_eq!(
        (status, &body["status"]),
        (StatusCode::OK, &json!("consent_required")),
        "{body}"
    );
    assert_eq!(stored(&app).await, ["color", "erp_id", "notes"]);
    let request = body["request"].as_str().unwrap().to_string();
    let (status, consent) = send(
        &app,
        "GET",
        &format!("/admin/oauth/requests/{request}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{consent}");
    let fields = &consent["custom_fields"];
    assert_eq!(names(&fields["removed"]), ["notes"], "{fields}");
    assert_eq!(fields["removed"][0]["type"], "multiline_text");
    assert_eq!(names(&fields["kept"]), ["erp_id", "color"], "{fields}");
    assert_eq!(names(&fields["added"]), Vec::<&str>::new());
    // Approving deletes it, with its values; the others keep theirs.
    let path = format!("/admin/oauth/requests/{request}/approve");
    let (status, answer) = send_as(&app, &owner, "POST", &path, None).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(stored(&app).await, ["color", "erp_id"]);
    assert_eq!(values_of(&app, &product).await, ["app_erp.erp_id"]);

    // A field's type never changes: refused on refresh and before any consent page.
    *doc.lock().unwrap() = manifest(&base, json!([erp_id("integer"), color()]));
    let (status, body) = send(&app, "POST", "/admin/apps/app_erp/refresh", None).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::UNPROCESSABLE_ENTITY, "manifest_invalid"),
        "{body}"
    );
    assert_eq!(
        body["details"]["errors"][0]["path"],
        "custom_fields[0].type"
    );
    let query = super::oauth::encode(&[
        ("response_type", "code"),
        ("client_id", &format!("{base}/hypha-app.json")),
        ("redirect_uri", &format!("{base}/callback")),
        ("code_challenge", CHALLENGE),
        ("code_challenge_method", "S256"),
    ]);
    let (status, location, body) =
        raw(&app, "GET", &format!("/oauth/authorize?{query}"), None).await;
    assert_eq!(
        (status, location, code(&body)),
        (StatusCode::BAD_REQUEST, None, "manifest_invalid"),
        "{body}"
    );
    assert_eq!(
        body["details"]["errors"][0]["path"],
        "custom_fields[0].type"
    );
    assert_eq!(stored(&app).await, ["color", "erp_id"]);
}

#[tokio::test]
async fn uninstalling_keeps_an_apps_fields_and_removing_the_app_deletes_them() {
    let app = oauth_app().await;
    let (base, _) = erp_app(json!([erp_id("text")])).await;
    install(&app, &base, None).await;
    let product = create_product(&app, "erp-kept").await;
    write_as_app(&app, &product, json!({"app_erp.erp_id": "E-2"})).await;

    let (status, _) = send(&app, "DELETE", "/admin/apps/app_erp", None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(stored(&app).await, ["erp_id"]);
    assert_eq!(values_of(&app, &product).await, ["app_erp.erp_id"]);

    let (status, _) = send(&app, "DELETE", "/admin/apps/app_erp", None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(stored(&app).await, Vec::<String>::new());
    assert_eq!(values_of(&app, &product).await, Vec::<String>::new());
}

#[tokio::test]
async fn a_private_apps_fields_are_created_when_it_is_approved() {
    let app = oauth_app().await;
    let owner = owner_session(&app).await;
    let mut manifest = manifest(
        "https://erp.internal.example",
        json!([erp_id("text"), notes()]),
    );
    manifest["redirect_uris"] = json!([]);
    let body = json!({ "manifest": manifest }).to_string();
    let (status, created) = send_as(&app, &owner, "POST", "/admin/apps/private", Some(&body)).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(
        names(&created["request"]["custom_fields"]["added"]),
        ["erp_id", "notes"]
    );
    assert_eq!(stored(&app).await, Vec::<String>::new());
    let request = created["request"]["id"].as_str().unwrap();
    let path = format!("/admin/oauth/requests/{request}/approve");
    let (status, answer) = send_as(&app, &owner, "POST", &path, None).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(stored(&app).await, ["erp_id", "notes"]);

    // A field the definitions refuse (a validation that doesn't fit its type) is the
    // manifest's problem, at its path, and nothing changes.
    let mut broken = manifest.clone();
    broken["custom_fields"][1]["validations"] = json!({ "max_length": 0, "min": 3 });
    let body = json!({ "manifest": broken }).to_string();
    let (status, created) = send_as(&app, &owner, "POST", "/admin/apps/private", Some(&body)).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let request = created["request"]["id"].as_str().unwrap();
    let path = format!("/admin/oauth/requests/{request}/approve");
    let (status, answer) = send_as(&app, &owner, "POST", &path, None).await;
    assert_eq!(
        (status, code(&answer)),
        (StatusCode::UNPROCESSABLE_ENTITY, "manifest_invalid"),
        "{answer}"
    );
    assert_eq!(answer["details"]["errors"][0]["path"], "custom_fields[1]");
}
