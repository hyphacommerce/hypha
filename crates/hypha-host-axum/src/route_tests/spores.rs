//! A host built with a spore (design note 0030): `deployment!` with the test spore
//! (`crate::test_spore`), its migrations, app, handler, configuration, settings and
//! routes. The stock build has none.

use hypha_apps::{AppKind, AppRepo, HasApps};
use hypha_core::{AppKey, EventHandlers};

use super::*;
use hypha_http::state::Storage;

use crate::test_spore::TestSpore;
use crate::{Deployment, PgPorts};

crate::deployment! { enum WithTestSpore {
    spores: [TestSpore],
    routes: [test_spore_routes],
} }

/// The test spore built from `toml` (its `hypha.config.toml`), on a fresh schema.
async fn storage_from(toml: &str) -> Result<crate::PgStorage<WithTestSpore>, String> {
    let config = Config::load(Some(toml), |_| None).map_err(|e| e.to_string())?;
    let runtime = crate::platform::runtime();
    let db = PgDatabase::connect(
        &fresh_schema().await,
        &config.database,
        runtime.clock.clone(),
        &crate::migrations::<WithTestSpore>(),
    )
    .await?;
    let env = HostEnv::new([7; 32], config.auth, runtime.clone());
    let outbound = hypha_http::outbound::OutboundPolicy::default();
    let client = crate::calls::client(outbound)?;
    let spores = hypha_http::spores::spore_host(&env, crate::calls::host_calls(client), outbound);
    let mail = Arc::new(MailHost::new(
        MailParts {
            sender: HostMail::log(),
            from: config.email.from.clone(),
            store: hypha_core::StoreId::from_uuid(uuid::Uuid::nil()),
            admin_url: None,
            order_tokens: env.order_tokens().clone(),
            media: None,
            queued: MailQueued::none(),
        },
        &runtime,
    ));
    crate::PgStorage::<WithTestSpore>::new(db, mail, &spores, &config.spores)
        .map_err(|e| e.to_string())
}

#[tokio::test]
async fn a_spore_reads_its_section_and_one_it_refuses_stops_the_host() {
    let storage = storage_from("[spores.test]\ngreeting = \"hi\"")
        .await
        .unwrap();
    let (spore,) = storage.spores();
    assert_eq!(spore.greeting, "hi");
    let refused = storage_from("[spores.test]\ngreting = \"hi\"").await.err();
    let refused = refused.unwrap_or_default();
    assert!(
        refused.starts_with("spore \"test\": unknown field `greting`"),
        "{refused}"
    );
}

#[tokio::test]
async fn a_host_built_with_a_spore_migrates_registers_and_runs_it() {
    let config = Config::defaults();
    let runtime = crate::platform::runtime();
    let url = fresh_schema().await;
    let db = PgDatabase::connect(
        &url,
        &config.database,
        runtime.clock.clone(),
        &crate::migrations::<WithTestSpore>(),
    )
    .await
    .unwrap();
    let env =
        HostEnv::new([7; 32], config.auth, runtime.clone()).with_hasher(Argon2id::new(8, 1, 1));
    // As the binary starts: the store, the spore's app, the owner, the location, search.
    let prepared =
        crate::start::prepare::<WithTestSpore>(db, env, HostMail::log(), runtime, config).await;
    let state = prepared.state;

    // Its handler runs beside core's.
    let names = EventHandlers::<PgPorts>::names(state.storage.handlers());
    assert!(names.contains(&"spore_test_noop"), "{names:?}");
    assert!(names.contains(&"variant_cleanup"), "{names:?}");
    assert_eq!(
        EventHandlers::<PgPorts>::wanting(state.storage.handlers(), "product.created"),
        ["search_index", "spore_test_noop"]
    );

    // Its app, enabled, its scopes its grant.
    let ports = state.public_ports().await.unwrap();
    let key = AppKey::of_spore("test").unwrap();
    let app = ports.apps().get(&key).await.unwrap().unwrap();
    assert_eq!(app.kind(), AppKind::Spore);
    assert_eq!(app.name(), "Test spore");
    assert!(app.enabled());
    assert_eq!(app.grant().unwrap().scopes(), &[Scope::ProductsRead]);

    // Its table, migrated after core's.
    let config = hypha_store_postgres::database_config(&url).unwrap();
    let (client, connection) = config.connect(tokio_postgres::NoTls).await.unwrap();
    tokio::spawn(connection);
    let row = client
        .query_one("SELECT to_regclass('spore_test_notes') IS NOT NULL", &[])
        .await
        .unwrap();
    assert!(row.get::<_, bool>(0));
}

#[tokio::test]
async fn the_spores_app_is_listed_with_its_kind() {
    let app = app_with_spores::<WithTestSpore>(Config::defaults()).await;
    let (status, list) = send(&app, "GET", "/admin/apps", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["items"][0]["key"], "spore_test", "{list}");
    assert_eq!(list["items"][0]["kind"], "spore", "{list}");
}

/// Turns the test spore on or off, as staff do.
async fn set_enabled(app: &TestApp<WithTestSpore>, enabled: bool) {
    let body = json!({ "enabled": enabled }).to_string();
    let (status, json) = send(app, "PATCH", "/admin/apps/spore_test", Some(&body)).await;
    assert_eq!(status, StatusCode::OK, "{json}");
}

#[tokio::test]
async fn a_spores_routes_serve_each_surface_and_answer_403_while_it_is_disabled() {
    let toml = "[spores.test]\ngreeting = \"hi\"";
    let app = app_with_spores::<WithTestSpore>(Config::load(Some(toml), |_| None).unwrap()).await;
    let pk = publishable(&app).await;

    // Public: no credentials.
    let (status, body) = send_with(&app, &[], "GET", "/spores/test/hello", None).await;
    assert_eq!((status, &body["greeting"]), (StatusCode::OK, &json!("hi")));
    // Admin: a staff session or secret key, not a publishable key.
    let (status, _) = send(&app, "GET", "/admin/spores/test/settings", None).await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = send_as(&app, &pk, "GET", "/admin/spores/test/settings", None).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    // Storefront: a publishable key.
    let (status, body) = send_as(&app, &pk, "GET", "/storefront/spores/test/greeting", None).await;
    assert_eq!(
        (status, &body["greeting"]),
        (StatusCode::OK, &json!("hi, shopper"))
    );

    set_enabled(&app, false).await;
    for (token, path) in [
        (None, "/spores/test/hello"),
        (Some(app.admin_key.as_str()), "/admin/spores/test/settings"),
        (Some(pk.as_str()), "/storefront/spores/test/greeting"),
    ] {
        let (status, body) = match token {
            Some(token) => send_as(&app, token, "GET", path, None).await,
            None => send_with(&app, &[], "GET", path, None).await,
        };
        assert_eq!(
            (status, code(&body)),
            (StatusCode::FORBIDDEN, "app_disabled"),
            "{path}"
        );
    }
    set_enabled(&app, true).await;
    let (status, _) = send_with(&app, &[], "GET", "/spores/test/hello", None).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn a_spores_public_routes_share_one_rate_limit() {
    let toml = "[rate_limits]\nspore_public = { burst = 2, per = \"1h\" }";
    let app = app_with_spores::<WithTestSpore>(Config::load(Some(toml), |_| None).unwrap()).await;
    for _ in 0..2 {
        let (status, _) = send_with(&app, &[], "GET", "/spores/test/hello", None).await;
        assert_eq!(status, StatusCode::OK);
    }
    let (status, body) = send_with(&app, &[], "GET", "/spores/test/hello", None).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::TOO_MANY_REQUESTS, "rate_limited")
    );
    // Credentialed routes don't count against it.
    let (status, _) = send(&app, "GET", "/admin/spores/test/settings", None).await;
    assert_eq!(status, StatusCode::OK);
}

#[test]
fn a_spores_routes_are_documented_under_its_tag() {
    use hypha_http::routes::openapi::{Doc, document_with};
    let admin =
        serde_json::to_value(document_with(Doc::Admin, WithTestSpore::spore_rows())).unwrap();
    let settings = &admin["paths"]["/admin/spores/test/settings"]["get"];
    assert_eq!(settings["tags"], json!(["spore_test"]));
    assert_eq!(settings["operationId"], "admin.spore_test.settings");
    let hello = &admin["paths"]["/spores/test/hello"]["get"];
    assert_eq!(hello["tags"], json!(["spore_test"]));
    assert_eq!(hello["security"], json!([{}]), "no credentials: {hello}");
    assert!(hello["responses"].get("401").is_none(), "{hello}");
    let storefront =
        serde_json::to_value(document_with(Doc::Storefront, WithTestSpore::spore_rows())).unwrap();
    assert!(storefront["paths"]["/storefront/spores/test/greeting"]["get"].is_object());
    // The stock build documents none.
    assert!(crate::Stock::spore_rows().is_empty());
    assert!(crate::check_routes::<WithTestSpore>().is_empty());
}

#[tokio::test]
async fn a_spores_settings_are_saved_sealed_and_read_back_by_the_spore() {
    let app = app_with_spores::<WithTestSpore>(Config::defaults()).await;
    let (status, before) = send(&app, "GET", "/admin/apps/spore_test/settings", None).await;
    assert_eq!(status, StatusCode::OK, "{before}");
    assert_eq!(
        before["schema"]["properties"]["api_key"]["x-hypha-secret"],
        true
    );
    assert_eq!(before["settings"], json!({ "api_key": { "set": false } }));
    assert_eq!(before["updated_at"], Value::Null);

    // The schema's rules, then the spore's own check, each with the field as its path.
    let body = json!({ "settings": { "mode": "staging" } }).to_string();
    let (status, error) = send(&app, "PUT", "/admin/apps/spore_test/settings", Some(&body)).await;
    assert_eq!(
        (status, code(&error)),
        (StatusCode::UNPROCESSABLE_ENTITY, "settings_invalid")
    );
    assert_eq!(error["details"]["errors"][0]["path"], "mode", "{error}");
    let body = json!({ "settings": { "mode": "test", "api_key": "nope" } }).to_string();
    let (status, error) = send(&app, "PUT", "/admin/apps/spore_test/settings", Some(&body)).await;
    assert_eq!(
        (status, code(&error)),
        (StatusCode::UNPROCESSABLE_ENTITY, "settings_invalid")
    );
    assert_eq!(
        error["details"]["errors"],
        json!([{ "path": "api_key", "message": "starts with key_" }])
    );

    // Saved: the secret answers as set, never its value.
    let body = json!({ "settings": { "mode": "test", "api_key": "key_123" } }).to_string();
    let (status, saved) = send(&app, "PUT", "/admin/apps/spore_test/settings", Some(&body)).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(
        saved["settings"],
        json!({ "mode": "test", "api_key": { "set": true } })
    );
    assert!(saved["updated_at"].is_string());
    assert!(!saved.to_string().contains("key_123"));
    // The spore reads it opened.
    let (status, seen) = send(&app, "GET", "/admin/spores/test/settings", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(seen, json!({ "mode": "test", "api_key": "key_123" }));
    // Leaving the secret out keeps it.
    let body = json!({ "settings": { "mode": "live" } }).to_string();
    let (status, _) = send(&app, "PUT", "/admin/apps/spore_test/settings", Some(&body)).await;
    assert_eq!(status, StatusCode::OK);
    let (_, seen) = send(&app, "GET", "/admin/spores/test/settings", None).await;
    assert_eq!(seen, json!({ "mode": "live", "api_key": "key_123" }));
}

#[tokio::test]
async fn only_a_built_in_spore_with_a_schema_has_settings_and_they_need_apps_scopes() {
    let app = app_with_spores::<WithTestSpore>(Config::defaults()).await;
    for path in [
        "/admin/apps/spore_stripe/settings",
        "/admin/apps/app_stripe/settings",
    ] {
        let (status, body) = send(&app, "GET", path, None).await;
        assert_eq!(
            (status, code(&body)),
            (StatusCode::NOT_FOUND, "app_not_found"),
            "{path}"
        );
    }
    let reader = key(
        &app,
        json!({ "kind": "secret", "name": "Reader", "scopes": ["apps:read"] }),
    )
    .await;
    let (status, _) = send_as(
        &app,
        &reader,
        "GET",
        "/admin/apps/spore_test/settings",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let body = json!({ "settings": { "mode": "test" } }).to_string();
    let (status, body) = send_as(
        &app,
        &reader,
        "PUT",
        "/admin/apps/spore_test/settings",
        Some(&body),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
}

#[tokio::test]
async fn a_disabled_spores_payment_provider_is_unavailable() {
    let app = app_with_spores::<WithTestSpore>(Config::defaults()).await;
    let method = |name: &str| json!({ "provider": "spore_test", "name": name }).to_string();
    let (status, body) = send(
        &app,
        "POST",
        "/admin/payment-methods",
        Some(&method("Test pay")),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    set_enabled(&app, false).await;
    let (status, body) = send(
        &app,
        "POST",
        "/admin/payment-methods",
        Some(&method("Test pay 2")),
    )
    .await;
    assert_eq!(code(&body), "payment_provider_unknown", "{status} {body}");
    set_enabled(&app, true).await;
    let (status, body) = send(
        &app,
        "POST",
        "/admin/payment-methods",
        Some(&method("Test pay 3")),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
}

#[tokio::test]
async fn a_disabled_spores_handlers_wait_on_the_retry_schedule() {
    use hypha_core::{HasOutbox, OutboxRepo, PageRequest};
    let app = app_with_spores::<WithTestSpore>(Config::defaults()).await;
    set_enabled(&app, false).await;
    create_product(&app, "spore-gated").await;
    hypha_http::jobs::dispatch(&app.state).await.unwrap();
    let ports = app.state.public_ports().await.unwrap();
    let deliveries = ports
        .outbox()
        .deliveries_to("handler:spore_test_noop", None, PageRequest::default())
        .await
        .unwrap();
    let delivery = deliveries
        .items
        .first()
        .expect("a delivery to the spore's handler");
    assert_eq!(delivery.status, hypha_core::DeliveryStatus::Pending);
    assert_eq!(
        delivery.last_error.as_deref(),
        Some("spore_test is disabled")
    );
}

#[tokio::test]
async fn a_spores_blocks_actions_and_settings_form_are_discovered_beside_apps() {
    let app = app_with_spores::<WithTestSpore>(Config::defaults()).await;
    let (status, found) = send(&app, "GET", "/admin/app-extensions", None).await;
    assert_eq!(status, StatusCode::OK, "{found}");
    assert_eq!(
        found["apps"],
        json!([{ "key": "spore_test", "name": "Test spore", "icon": null }])
    );
    assert_eq!(
        found["blocks"],
        json!([
            {
                "app": "spore_test", "id": "notes", "target": "product.details",
                "url": "/admin/app-extensions/blocks/spore_test/notes"
            },
            {
                "app": "spore_test", "id": "broken", "target": "product.details",
                "url": "/admin/app-extensions/blocks/spore_test/broken"
            }
        ])
    );
    assert_eq!(
        found["actions"],
        json!([{
            "app": "spore_test", "id": "greet", "label": "Greet", "target": "product.details",
            "input_schema": null, "point": null
        }])
    );
    assert_eq!(found["settings"][0]["app"], "spore_test");
    assert_eq!(found["settings"][0]["secret_fields"], json!(["api_key"]));
    assert_eq!(found["settings"][0]["proxy"], false);
    // By target: an order has none of the spore's.
    let (_, orders) = send(
        &app,
        "GET",
        "/admin/app-extensions?target=order.details",
        None,
    )
    .await;
    assert_eq!(orders["apps"], json!([]));

    // Disabled, it isn't discovered, and its block and action answer 403.
    set_enabled(&app, false).await;
    let (_, found) = send(&app, "GET", "/admin/app-extensions", None).await;
    assert_eq!(
        (&found["apps"], &found["blocks"], &found["settings"]),
        (&json!([]), &json!([]), &json!([]))
    );
    let product = create_product(&app, "spore-ui-off").await;
    let path = format!("/admin/app-extensions/blocks/spore_test/notes?product_id={product}");
    let (status, body) = send(&app, "GET", &path, None).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::FORBIDDEN, "app_disabled")
    );
}

#[tokio::test]
async fn a_spores_block_and_action_are_answered_in_process() {
    let app = app_with_spores::<WithTestSpore>(Config::defaults()).await;
    let product = create_product(&app, "spore-ui").await;

    let path = format!("/admin/app-extensions/blocks/spore_test/notes?product_id={product}");
    let (status, block) = send(&app, "GET", &path, None).await;
    assert_eq!(status, StatusCode::OK, "{block}");
    assert_eq!(
        (&block["app"], &block["id"], &block["target"]),
        (
            &json!("spore_test"),
            &json!("notes"),
            &json!("product.details")
        )
    );
    assert_eq!(block["tree"]["version"], 1);
    assert!(
        block["tree"]["root"]["children"][0]["value"]
            .as_str()
            .unwrap()
            .starts_with("Notes on "),
        "{block}"
    );

    // A tree that isn't v1 is the spore's failure, as an app's would be.
    let path = format!("/admin/app-extensions/blocks/spore_test/broken?product_id={product}");
    let (status, body) = send(&app, "GET", &path, None).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::BAD_GATEWAY, "app_unavailable"),
        "{body}"
    );
    // The target's context is checked, and unknown blocks are 404.
    let (status, body) = send(
        &app,
        "GET",
        "/admin/app-extensions/blocks/spore_test/notes",
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
    let path = format!("/admin/app-extensions/blocks/spore_test/nope?product_id={product}");
    let (status, body) = send(&app, "GET", &path, None).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::NOT_FOUND, "app_extension_not_found")
    );

    // The action runs as the spore, with its scopes as far as the caller holds them.
    let body = json!({ "context": { "product_id": product } }).to_string();
    let (status, answer) = send(
        &app,
        "POST",
        "/admin/app-extensions/actions/spore_test/greet",
        Some(&body),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(answer["message"], "hello, as [ProductsRead]");
    assert_eq!(answer["refresh"], true);

    // A caller who can't read products can't use a product block.
    let reader = key(
        &app,
        json!({ "kind": "secret", "name": "Apps only", "scopes": ["apps:read"] }),
    )
    .await;
    let path = format!("/admin/app-extensions/blocks/spore_test/notes?product_id={product}");
    let (status, body) = send_as(&app, &reader, "GET", &path, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
}
