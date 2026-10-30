//! What OAuth does about an app's custom fields: it refuses a manifest that retypes one
//! before any consent page, says which manifest a write applies (the HTTP layer syncs the
//! fields from it in the same batch), and lets a refresh that drops data wait for staff.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a failing test panics"
)]
use std::collections::HashMap;

use hypha_apps::{AppRepo, FetchError, HasApps, ManifestFetcher};
use hypha_core::{Actor, AppKey, Ctx, HasBatch, Scope, UserId};
use hypha_oauth::{
    Approval, AuthorizeError, AuthorizeInput, Authorized, Refreshed, UnredirectableError, approve,
    authorize, pkce, prepare_approval, prepare_refresh,
};
use hypha_testkit::{InMemoryPorts, TestEnv};
use pollster::block_on;
use serde_json::{Value, json};
use uuid::Uuid;

const CLIENT: &str = "https://erp.example/hypha-app.json";
const VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";

struct Manifests(HashMap<String, Vec<u8>>);

impl ManifestFetcher for Manifests {
    async fn fetch(&self, url: &str) -> Result<Vec<u8>, String> {
        self.0
            .get(url)
            .cloned()
            .ok_or_else(|| "answered HTTP 404".to_string())
    }
}

fn serving(scopes: &[&str], fields: Value) -> Manifests {
    let doc = json!({
        "key": "app_erp",
        "name": "ERP",
        "version": "1.0.0",
        "app_url": "https://erp.example",
        "redirect_uris": ["/callback"],
        "scopes": scopes,
        "privacy_url": "/privacy",
        "custom_fields": fields
    });
    Manifests(HashMap::from([(
        CLIENT.to_string(),
        doc.to_string().into_bytes(),
    )]))
}

fn field(key: &str, field_type: &str) -> Value {
    json!({ "owner_type": "product", "key": key, "type": field_type, "name": key })
}

fn owner() -> Ctx {
    Ctx::new(
        Actor::User(UserId::from_uuid(Uuid::from_u128(1))),
        Scope::ALL.to_vec(),
    )
}

fn input() -> AuthorizeInput {
    AuthorizeInput {
        response_type: Some("code".to_string()),
        client_id: Some(CLIENT.to_string()),
        redirect_uri: Some("https://erp.example/callback".to_string()),
        scope: None,
        state: None,
        code_challenge: Some(pkce::challenge_of(VERIFIER)),
        code_challenge_method: Some("S256".to_string()),
    }
}

/// Installs `app_erp` from `fetch`'s manifest.
fn install(ports: &InMemoryPorts, env: &TestEnv, fetch: &Manifests) {
    let Authorized::Consent(request) = block_on(authorize(ports, fetch, env, input())).unwrap()
    else {
        panic!("expected a consent page");
    };
    let approval = block_on(approve(&owner(), ports, env, request.id())).unwrap();
    assert!(matches!(approval, Approval::Redirect(_)));
}

fn key() -> AppKey {
    AppKey::new("app_erp").unwrap()
}

#[test]
fn authorize_refuses_a_manifest_that_retypes_a_field_before_any_consent() {
    let (ports, env) = (InMemoryPorts::default(), TestEnv::default());
    install(
        &ports,
        &env,
        &serving(&["products:read"], json!([field("erp_id", "text")])),
    );
    let retyped = serving(
        &["products:read", "orders:read"],
        json!([field("erp_id", "integer")]),
    );
    let refused = block_on(authorize(&ports, &retyped, &env, input()));
    let Err(AuthorizeError::Unredirectable(UnredirectableError::Manifest(FetchError::Invalid(e)))) =
        refused
    else {
        panic!("expected an invalid manifest, got {refused:?}");
    };
    assert_eq!(e.problems[0].path, "custom_fields[0].type");
}

#[test]
fn an_approval_is_staged_with_the_manifest_it_applies_and_nothing_stored_until_committed() {
    let (ports, env) = (InMemoryPorts::default(), TestEnv::default());
    let fetch = serving(&["products:read"], json!([field("erp_id", "text")]));
    let Authorized::Consent(request) = block_on(authorize(&ports, &fetch, &env, input())).unwrap()
    else {
        panic!("expected a consent page");
    };
    let prepared = block_on(prepare_approval(&owner(), &ports, &env, request.id())).unwrap();
    assert_eq!(prepared.applies.as_ref(), request.manifest());
    assert_eq!(block_on(ports.apps().get(&key())), Ok(None));
    block_on(ports.commit(prepared.batch)).unwrap();
    assert!(block_on(ports.apps().get(&key())).unwrap().is_some());
}

#[test]
fn a_refresh_that_drops_data_waits_for_staff_and_one_that_doesnt_applies_its_fields() {
    let (ports, env) = (InMemoryPorts::default(), TestEnv::default());
    install(
        &ports,
        &env,
        &serving(
            &["products:read"],
            json!([field("erp_id", "text"), field("notes", "text")]),
        ),
    );
    let fewer = serving(&["products:read"], json!([field("erp_id", "text")]));

    let prepared = block_on(prepare_refresh(
        &owner(),
        &ports,
        &fewer,
        &env,
        "app_erp",
        |_| true,
    ))
    .unwrap();
    assert_eq!(prepared.applies, None);
    let Refreshed::ConsentRequired(_, request) = &prepared.answer else {
        panic!("expected a consent, got {:?}", prepared.answer);
    };
    assert_eq!(request.scopes(), &[Scope::ProductsRead]);
    block_on(ports.commit(prepared.batch)).unwrap();

    let prepared = block_on(prepare_refresh(
        &owner(),
        &ports,
        &fewer,
        &env,
        "app_erp",
        |_| false,
    ))
    .unwrap();
    assert!(matches!(prepared.answer, Refreshed::Updated(_)));
    let applied = prepared.applies.unwrap();
    assert_eq!(applied.custom_fields().len(), 1);
}
