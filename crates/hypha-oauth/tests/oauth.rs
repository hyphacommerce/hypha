//! The OAuth flow's commands on the in-memory repos (design note 0029, §4.1).

use std::collections::HashMap;

use hypha_apps::{AppError, AppRepo, ManifestFetcher};
use hypha_core::{Actor, AppKey, AuthError, Clock, Ctx, OAuthRequestId, Scope, UserId};
use hypha_oauth::{
    Approval, AuthorizeError, AuthorizeInput, Authorized, CodeExchange, CommandError, Refresh,
    RequestStatus, TokenError, Uninstalled, UnredirectableError, approve,
    authenticate_access_token, authorize, deny, exchange_code, narrow_grant, pkce, refresh,
    refresh_manifest, request_private_app, revoke, uninstall,
};
use hypha_testkit::{InMemoryPorts, TestEnv};
use pollster::block_on;
use uuid::Uuid;

const CLIENT: &str = "https://shop-app.example/hypha-app.json";
const REDIRECT: &str = "https://shop-app.example/callback";
const VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";

/// Serves manifests from memory.
struct Manifests(HashMap<String, Vec<u8>>);

impl ManifestFetcher for Manifests {
    async fn fetch(&self, url: &str) -> Result<Vec<u8>, String> {
        self.0
            .get(url)
            .cloned()
            .ok_or_else(|| "answered HTTP 404".to_string())
    }
}

fn manifest(scopes: &[&str]) -> Vec<u8> {
    serde_json::json!({
        "key": "app_shop",
        "name": "Shop helper",
        "version": "1.0.0",
        "app_url": "https://shop-app.example",
        "redirect_uris": ["/callback"],
        "scopes": scopes,
        "scope_reasons": { "customer_data:email": "receipts" },
        "privacy_url": "/privacy",
        "webhooks": { "url": "/hooks", "events": ["order.*"] }
    })
    .to_string()
    .into_bytes()
}

fn fetcher(scopes: &[&str]) -> Manifests {
    Manifests(HashMap::from([(CLIENT.to_string(), manifest(scopes))]))
}

const ALL: &[&str] = &["orders:read", "customers:read", "customer_data:email"];

fn owner() -> Ctx {
    Ctx::new(
        Actor::User(UserId::from_uuid(Uuid::from_u128(1))),
        Scope::ALL.to_vec(),
    )
}

fn input(scope: Option<&str>) -> AuthorizeInput {
    AuthorizeInput {
        response_type: Some("code".to_string()),
        client_id: Some(CLIENT.to_string()),
        redirect_uri: Some(REDIRECT.to_string()),
        scope: scope.map(str::to_string),
        state: Some("st4te".to_string()),
        code_challenge: Some(pkce::challenge_of(VERIFIER)),
        code_challenge_method: Some("S256".to_string()),
    }
}

/// The query parameter `name` of `url`.
fn param(url: &str, name: &str) -> Option<String> {
    url::Url::parse(url)
        .unwrap()
        .query_pairs()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.into_owned())
}

fn pending(
    ports: &InMemoryPorts,
    env: &TestEnv,
    scope: Option<&str>,
    fetch: &Manifests,
) -> OAuthRequestId {
    match block_on(authorize(ports, fetch, env, input(scope))).unwrap() {
        Authorized::Consent(request) => request.id(),
        other => panic!("expected a consent, got {other:?}"),
    }
}

fn exchange(code: &str) -> CodeExchange {
    CodeExchange {
        code: code.to_string(),
        code_verifier: VERIFIER.to_string(),
        client_id: CLIENT.to_string(),
        redirect_uri: REDIRECT.to_string(),
    }
}

/// Installs the app with `ALL` and returns the first tokens' answer.
fn install(ports: &InMemoryPorts, env: &TestEnv) -> hypha_oauth::IssuedTokens {
    let id = pending(ports, env, None, &fetcher(ALL));
    let location = redirect(block_on(approve(&owner(), ports, env, id)));
    let code = param(&location, "code").unwrap();
    block_on(exchange_code(ports, env, exchange(&code))).unwrap()
}

fn later(env: &TestEnv, secs: i64) {
    env.clock.set(env.clock.now().plus_secs(secs));
}

#[test]
fn the_whole_dance_installs_the_app_and_its_tokens_act_as_it() {
    let (ports, env) = (InMemoryPorts::default(), TestEnv::default());
    let id = pending(&ports, &env, None, &fetcher(ALL));
    let location = redirect(block_on(approve(&owner(), &ports, &env, id)));
    assert!(location.starts_with(REDIRECT), "{location}");
    assert_eq!(param(&location, "state").as_deref(), Some("st4te"));
    let code = param(&location, "code").unwrap();
    assert!(code.starts_with("hypha_ac_"));
    // The app has no secret yet: `app.installed` waits for the exchange.
    assert_eq!(ports.outbox.events().len(), 0);

    let tokens = block_on(exchange_code(&ports, &env, exchange(&code))).unwrap();
    assert!(tokens.access_token.expose().starts_with("hypha_at_"));
    assert!(tokens.refresh_token.expose().starts_with("hypha_rt_"));
    assert_eq!(tokens.expires_in, 3600);
    assert!(
        tokens
            .app_secret
            .as_deref()
            .unwrap()
            .starts_with("hypha_as_")
    );
    assert_eq!(
        tokens.scopes,
        vec![
            Scope::CustomersRead,
            Scope::OrdersRead,
            Scope::CustomerDataEmail
        ]
    );

    let app = block_on(ports.apps.get(&AppKey::new("app_shop").unwrap()))
        .unwrap()
        .unwrap();
    assert!(app.is_installed());
    assert_eq!(
        app.webhook().unwrap().url.as_str(),
        "https://shop-app.example/hooks"
    );
    let ctx = block_on(authenticate_access_token(
        &ports,
        &env.clock,
        tokens.access_token.expose(),
    ))
    .unwrap();
    assert_eq!(ctx.actor(), Actor::App(app.id()));
    assert_eq!(ctx.require(Scope::OrdersRead), Ok(()));
    assert_eq!(
        ctx.require(Scope::ProductsWrite),
        Err(AuthError::AppNotAuthorized(Scope::ProductsWrite))
    );
    let types: Vec<String> = ports
        .outbox
        .events()
        .into_iter()
        .map(|e| e.event_type)
        .collect();
    assert_eq!(types, vec!["app.installed"]);
}

#[test]
fn a_code_works_once_and_a_second_try_revokes_what_the_first_gave() {
    let (ports, env) = (InMemoryPorts::default(), TestEnv::default());
    let id = pending(&ports, &env, None, &fetcher(ALL));
    let location = redirect(block_on(approve(&owner(), &ports, &env, id)));
    let code = param(&location, "code").unwrap();
    let first = block_on(exchange_code(&ports, &env, exchange(&code))).unwrap();
    assert_eq!(
        block_on(exchange_code(&ports, &env, exchange(&code))),
        Err(TokenError::InvalidGrant(
            "the code was used already".to_string()
        ))
    );
    assert_eq!(
        block_on(authenticate_access_token(
            &ports,
            &env.clock,
            first.access_token.expose()
        )),
        Err(CommandError::InvalidToken)
    );
}

#[test]
fn a_code_needs_its_verifier_redirect_and_client_within_a_minute() {
    let (ports, env) = (InMemoryPorts::default(), TestEnv::default());
    let id = pending(&ports, &env, None, &fetcher(ALL));
    let location = redirect(block_on(approve(&owner(), &ports, &env, id)));
    let code = param(&location, "code").unwrap();
    let grant = |why: &str| Err(TokenError::InvalidGrant(why.to_string()));

    let mut wrong = exchange(&code);
    wrong.code_verifier = format!("{}x", &VERIFIER[..VERIFIER.len() - 1]);
    assert_eq!(
        block_on(exchange_code(&ports, &env, wrong)),
        grant("code_verifier doesn't match the code_challenge")
    );
    let mut wrong = exchange(&code);
    wrong.redirect_uri = "https://shop-app.example/other".to_string();
    assert_eq!(
        block_on(exchange_code(&ports, &env, wrong)),
        grant("redirect_uri differs from the authorization's")
    );
    let mut wrong = exchange(&code);
    wrong.client_id = "https://evil.example/hypha-app.json".to_string();
    assert_eq!(
        block_on(exchange_code(&ports, &env, wrong)),
        grant("the code was issued to another client")
    );
    later(&env, 60);
    assert_eq!(
        block_on(exchange_code(&ports, &env, exchange(&code))),
        grant("the code has expired")
    );
    assert_eq!(
        block_on(exchange_code(&ports, &env, exchange("hypha_ac_nope"))),
        grant("the code is invalid")
    );
}

#[test]
fn refreshing_rotates_and_reusing_a_rotated_token_revokes_everything() {
    let (ports, env) = (InMemoryPorts::default(), TestEnv::default());
    let first = install(&ports, &env);
    let again = |token: &str| Refresh {
        refresh_token: token.to_string(),
        client_id: Some(CLIENT.to_string()),
        scope: None,
    };
    let second = block_on(refresh(&ports, &env, again(first.refresh_token.expose()))).unwrap();
    assert_eq!(second.app_secret, None);
    assert_ne!(second.access_token, first.access_token);
    // The first access token still works until it expires; the new one too.
    for token in [&first.access_token, &second.access_token] {
        assert!(
            block_on(authenticate_access_token(
                &ports,
                &env.clock,
                token.expose()
            ))
            .is_ok()
        );
    }

    // The first refresh token, used again: it was copied.
    assert_eq!(
        block_on(refresh(&ports, &env, again(first.refresh_token.expose()))),
        Err(TokenError::InvalidGrant(
            "the refresh token was used already: every token of the app is revoked".to_string()
        ))
    );
    assert_eq!(
        block_on(authenticate_access_token(
            &ports,
            &env.clock,
            second.access_token.expose()
        )),
        Err(CommandError::InvalidToken)
    );
    assert_eq!(
        block_on(refresh(&ports, &env, again(second.refresh_token.expose()))),
        Err(TokenError::InvalidGrant(
            "the refresh token was revoked".to_string()
        ))
    );
}

#[test]
fn access_tokens_expire_after_an_hour() {
    let (ports, env) = (InMemoryPorts::default(), TestEnv::default());
    let tokens = install(&ports, &env);
    later(&env, 3599);
    assert!(
        block_on(authenticate_access_token(
            &ports,
            &env.clock,
            tokens.access_token.expose()
        ))
        .is_ok()
    );
    later(&env, 1);
    assert_eq!(
        block_on(authenticate_access_token(
            &ports,
            &env.clock,
            tokens.access_token.expose()
        )),
        Err(CommandError::InvalidToken)
    );
}

#[test]
fn revoking_a_token_revokes_its_family() {
    let (ports, env) = (InMemoryPorts::default(), TestEnv::default());
    let tokens = install(&ports, &env);
    block_on(revoke(&ports, &env.clock, tokens.refresh_token.expose())).unwrap();
    assert_eq!(
        block_on(authenticate_access_token(
            &ports,
            &env.clock,
            tokens.access_token.expose()
        )),
        Err(CommandError::InvalidToken)
    );
    // Unknown tokens are fine.
    assert_eq!(block_on(revoke(&ports, &env.clock, "nonsense")), Ok(()));
}

#[test]
fn staff_must_hold_every_scope_they_approve() {
    let (ports, env) = (InMemoryPorts::default(), TestEnv::default());
    let id = pending(&ports, &env, None, &fetcher(ALL));
    let manager = Ctx::new(
        Actor::User(UserId::from_uuid(Uuid::from_u128(2))),
        vec![Scope::AppsWrite, Scope::OrdersRead, Scope::CustomersRead],
    );
    assert_eq!(
        block_on(approve(&manager, &ports, &env, id)),
        Err(CommandError::CannotGrant(Scope::CustomerDataEmail))
    );
    let reader = Ctx::new(manager.actor(), vec![Scope::AppsRead]);
    assert_eq!(
        block_on(approve(&reader, &ports, &env, id)),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::AppsWrite
        )))
    );
}

#[test]
fn a_request_waits_ten_minutes_for_one_answer() {
    let (ports, env) = (InMemoryPorts::default(), TestEnv::default());
    let id = pending(&ports, &env, None, &fetcher(ALL));
    let location = block_on(deny(&owner(), &ports, &env.clock, id))
        .unwrap()
        .unwrap();
    assert_eq!(param(&location, "error").as_deref(), Some("access_denied"));
    assert_eq!(param(&location, "state").as_deref(), Some("st4te"));
    assert_eq!(
        block_on(approve(&owner(), &ports, &env, id)),
        Err(CommandError::RequestDecided(id))
    );

    let late = pending(&ports, &env, None, &fetcher(ALL));
    later(&env, 600);
    assert_eq!(
        block_on(approve(&owner(), &ports, &env, late)),
        Err(CommandError::RequestExpired(late))
    );
    let unknown = OAuthRequestId::from_uuid(Uuid::from_u128(999));
    assert_eq!(
        block_on(approve(&owner(), &ports, &env, unknown)),
        Err(CommandError::RequestNotFound(unknown))
    );
}

#[test]
fn more_scopes_need_consent_and_fewer_apply_at_once() {
    let (ports, env) = (InMemoryPorts::default(), TestEnv::default());
    let first = pending(&ports, &env, Some("orders:read"), &fetcher(ALL));
    let location = redirect(block_on(approve(&owner(), &ports, &env, first)));
    let code = param(&location, "code").unwrap();
    block_on(exchange_code(&ports, &env, exchange(&code))).unwrap();

    // More: a request showing what it had.
    let more = match block_on(authorize(&ports, &fetcher(ALL), &env, input(None))).unwrap() {
        Authorized::Consent(request) => request,
        other => panic!("{other:?}"),
    };
    assert_eq!(more.previous_scopes(), Some(&[Scope::OrdersRead][..]));
    assert_eq!(more.status(), RequestStatus::Pending);
    let key = AppKey::new("app_shop").unwrap();
    let granted = |ports: &InMemoryPorts| {
        block_on(ports.apps.get(&key))
            .unwrap()
            .unwrap()
            .grant()
            .unwrap()
            .scopes()
            .to_vec()
    };
    assert_eq!(
        granted(&ports),
        vec![Scope::OrdersRead],
        "the old grant stands"
    );
    let location = redirect(block_on(approve(&owner(), &ports, &env, more.id())));
    let code = param(&location, "code").unwrap();
    let tokens = block_on(exchange_code(&ports, &env, exchange(&code))).unwrap();
    assert_eq!(tokens.app_secret, None, "only the first install shows it");
    assert_eq!(granted(&ports).len(), 3);

    // Fewer: straight back to the app, and the grant narrows.
    let location = match block_on(authorize(
        &ports,
        &fetcher(ALL),
        &env,
        input(Some("orders:read")),
    ))
    .unwrap()
    {
        Authorized::Redirect(location) => location,
        other => panic!("{other:?}"),
    };
    assert!(param(&location, "code").is_some());
    assert_eq!(granted(&ports), vec![Scope::OrdersRead]);
    let types: Vec<String> = ports
        .outbox
        .events()
        .into_iter()
        .map(|e| e.event_type)
        .collect();
    assert_eq!(
        types,
        vec!["app.installed", "app.grant_changed", "app.grant_changed"]
    );
}

#[test]
fn staff_narrow_a_grant_but_cannot_widen_it() {
    let (ports, env) = (InMemoryPorts::default(), TestEnv::default());
    let tokens = install(&ports, &env);
    let app = block_on(narrow_grant(
        &owner(),
        &ports,
        &env.clock,
        "app_shop",
        &[Scope::OrdersRead],
    ))
    .unwrap();
    assert_eq!(app.grant().unwrap().scopes(), &[Scope::OrdersRead]);
    let ctx = block_on(authenticate_access_token(
        &ports,
        &env.clock,
        tokens.access_token.expose(),
    ))
    .unwrap();
    assert_eq!(
        ctx.scopes(),
        &[Scope::OrdersRead],
        "at once, for tokens held already"
    );
    assert_eq!(
        block_on(narrow_grant(
            &owner(),
            &ports,
            &env.clock,
            "app_shop",
            &[Scope::ProductsRead]
        )),
        Err(CommandError::App(AppError::GrantNotNarrower(
            Scope::ProductsRead
        )))
    );
}

#[test]
fn uninstalling_revokes_every_token_then_deleting_removes_the_app() {
    let (ports, env) = (InMemoryPorts::default(), TestEnv::default());
    let tokens = install(&ports, &env);
    assert_eq!(
        block_on(uninstall(&owner(), &ports, &env.clock, "app_shop")),
        Ok(Uninstalled::Uninstalled)
    );
    assert_eq!(
        block_on(authenticate_access_token(
            &ports,
            &env.clock,
            tokens.access_token.expose()
        )),
        Err(CommandError::InvalidToken)
    );
    let key = AppKey::new("app_shop").unwrap();
    let app = block_on(ports.apps.get(&key)).unwrap().unwrap();
    assert!(app.uninstalled_at().is_some());
    let last = ports.outbox.events().pop().unwrap();
    assert_eq!(last.event_type, "app.uninstalled");

    // Installing again: a new secret, shown again.
    let again = install(&ports, &env);
    assert!(again.app_secret.is_some());
    assert_ne!(again.app_secret, tokens.app_secret);

    block_on(uninstall(&owner(), &ports, &env.clock, "app_shop")).unwrap();
    assert_eq!(
        block_on(uninstall(&owner(), &ports, &env.clock, "app_shop")),
        Ok(Uninstalled::Deleted)
    );
    assert_eq!(block_on(ports.apps.get(&key)), Ok(None));
}

#[test]
fn an_untrusted_client_or_redirect_gets_a_page_and_the_rest_go_back_to_the_app() {
    let (ports, env) = (InMemoryPorts::default(), TestEnv::default());
    let fetch = fetcher(ALL);
    let mut bad = input(None);
    bad.redirect_uri = Some("https://evil.example/cb".to_string());
    assert_eq!(
        block_on(authorize(&ports, &fetch, &env, bad)),
        Err(AuthorizeError::Unredirectable(
            UnredirectableError::RedirectUriNotListed("https://evil.example/cb".to_string())
        ))
    );
    let mut unknown = input(None);
    unknown.client_id = Some("https://nowhere.example/app.json".to_string());
    assert!(matches!(
        block_on(authorize(&ports, &fetch, &env, unknown)),
        Err(AuthorizeError::Unredirectable(
            UnredirectableError::Manifest(_)
        ))
    ));

    let mut no_pkce = input(None);
    no_pkce.code_challenge = None;
    match block_on(authorize(&ports, &fetch, &env, no_pkce)) {
        Err(AuthorizeError::Redirect {
            location, error, ..
        }) => {
            assert_eq!(error, "invalid_request");
            assert_eq!(
                param(&location, "error").as_deref(),
                Some("invalid_request")
            );
            assert_eq!(param(&location, "state").as_deref(), Some("st4te"));
        }
        other => panic!("{other:?}"),
    }
    let mut plain = input(None);
    plain.code_challenge_method = Some("plain".to_string());
    assert!(matches!(
        block_on(authorize(&ports, &fetch, &env, plain)),
        Err(AuthorizeError::Redirect {
            error: "invalid_request",
            ..
        })
    ));
    assert!(matches!(
        block_on(authorize(&ports, &fetch, &env, input(Some("orders:write")))),
        Err(AuthorizeError::Redirect {
            error: "invalid_scope",
            ..
        })
    ));
}

#[test]
fn a_disabled_apps_tokens_stop_working() {
    let (ports, env) = (InMemoryPorts::default(), TestEnv::default());
    let tokens = install(&ports, &env);
    let key = AppKey::new("app_shop").unwrap();
    let mut app = block_on(ports.apps.get(&key)).unwrap().unwrap();
    app.set_enabled(false);
    block_on(ports.apps.update(&mut app)).unwrap();
    assert_eq!(
        block_on(authenticate_access_token(
            &ports,
            &env.clock,
            tokens.access_token.expose()
        )),
        Err(CommandError::InvalidToken)
    );
}

/// Where an approval sends the browser (a public app's).
fn redirect(approval: Result<Approval, CommandError>) -> String {
    match approval.unwrap() {
        Approval::Redirect(location) => location,
        other => panic!("expected a redirect, got {other:?}"),
    }
}

/// A private app's manifest: uploaded, so it lists no redirect URIs.
fn private_manifest(scopes: &[&str]) -> Vec<u8> {
    let reasons = if scopes.contains(&"customer_data:address") {
        serde_json::json!({ "customer_data:address": "quotes by postal code" })
    } else {
        serde_json::json!({})
    };
    serde_json::json!({
        "key": "app_rates",
        "name": "Our rates",
        "version": "1.0.0",
        "app_url": "https://rates.internal.example",
        "redirect_uris": [],
        "scopes": scopes,
        "scope_reasons": reasons,
        "privacy_url": "/privacy"
    })
    .to_string()
    .into_bytes()
}

fn credentials(approval: Result<Approval, CommandError>) -> hypha_oauth::IssuedTokens {
    match approval.unwrap() {
        Approval::Credentials { tokens, .. } => *tokens,
        other => panic!("expected credentials, got {other:?}"),
    }
}

#[test]
fn an_uploaded_manifest_installs_a_private_app_with_its_tokens_on_approval() {
    let (ports, env) = (InMemoryPorts::default(), TestEnv::default());
    let manifest = private_manifest(&["shipping:read", "customer_data:address"]);
    let request = block_on(request_private_app(&owner(), &ports, &env, &manifest)).unwrap();
    assert!(request.is_private());
    assert_eq!(request.client_id(), "urn:hypha:private:app_rates");
    assert_eq!(request.redirect_uri(), "");
    assert_eq!(request.status(), RequestStatus::Pending);

    let tokens = credentials(block_on(approve(&owner(), &ports, &env, request.id())));
    assert!(
        tokens
            .app_secret
            .as_deref()
            .is_some_and(|s| s.starts_with("hypha_as_"))
    );
    assert_eq!(
        tokens.scopes,
        vec![Scope::ShippingRead, Scope::CustomerDataAddress]
    );
    let ctx = block_on(authenticate_access_token(
        &ports,
        &env.clock,
        tokens.access_token.expose(),
    ))
    .unwrap();
    assert!(matches!(ctx.actor(), Actor::App(_)));
    assert!(ctx.has_scope(Scope::CustomerDataAddress));
    // The refresh token rotates like any app's.
    let refreshed = block_on(refresh(
        &ports,
        &env,
        Refresh {
            refresh_token: tokens.refresh_token.expose().to_string(),
            client_id: Some("urn:hypha:private:app_rates".to_string()),
            scope: None,
        },
    ))
    .unwrap();
    assert_eq!(refreshed.app_secret, None);
    let app = block_on(ports.apps.get(&AppKey::new("app_rates").unwrap()))
        .unwrap()
        .unwrap();
    assert!(app.is_installed());
    assert_eq!(
        app.manifest().map(|m| m.url.as_str()),
        Some("urn:hypha:private:app_rates")
    );
}

#[test]
fn uploading_again_changes_a_private_apps_grant_and_issues_new_tokens_without_the_secret() {
    let (ports, env) = (InMemoryPorts::default(), TestEnv::default());
    let first = block_on(request_private_app(
        &owner(),
        &ports,
        &env,
        &private_manifest(&["shipping:read"]),
    ))
    .unwrap();
    credentials(block_on(approve(&owner(), &ports, &env, first.id())));

    let more = block_on(request_private_app(
        &owner(),
        &ports,
        &env,
        &private_manifest(&["shipping:read", "customer_data:address"]),
    ))
    .unwrap();
    assert_eq!(more.previous_scopes(), Some(&[Scope::ShippingRead][..]));
    let tokens = credentials(block_on(approve(&owner(), &ports, &env, more.id())));
    assert_eq!(tokens.app_secret, None, "the app keeps its secret");
    assert_eq!(
        tokens.scopes,
        vec![Scope::ShippingRead, Scope::CustomerDataAddress]
    );
}

#[test]
fn a_private_upload_cant_take_a_served_apps_key_nor_be_refreshed() {
    let (ports, env) = (InMemoryPorts::default(), TestEnv::default());
    install(&ports, &env);
    let mut taken: serde_json::Value =
        serde_json::from_slice(&private_manifest(&["orders:read"])).unwrap();
    taken["key"] = "app_shop".into();
    assert_eq!(
        block_on(request_private_app(
            &owner(),
            &ports,
            &env,
            taken.to_string().as_bytes()
        )),
        Err(CommandError::InstalledFromUrl(
            AppKey::new("app_shop").unwrap()
        ))
    );

    let request = block_on(request_private_app(
        &owner(),
        &ports,
        &env,
        &private_manifest(&["shipping:read"]),
    ))
    .unwrap();
    credentials(block_on(approve(&owner(), &ports, &env, request.id())));
    assert_eq!(
        block_on(refresh_manifest(
            &owner(),
            &ports,
            &fetcher(ALL),
            &env,
            "app_rates"
        )),
        Err(CommandError::PrivateApp(AppKey::new("app_rates").unwrap()))
    );
}

#[test]
fn an_upload_is_checked_like_a_manifest_and_needs_apps_write() {
    let (ports, env) = (InMemoryPorts::default(), TestEnv::default());
    let reader = Ctx::new(owner().actor(), vec![Scope::AppsRead]);
    assert_eq!(
        block_on(request_private_app(
            &reader,
            &ports,
            &env,
            &private_manifest(&[])
        )),
        Err(CommandError::Auth(AuthError::MissingScope(
            Scope::AppsWrite
        )))
    );
    // Personal data needs a reason, uploaded or not.
    let bad = block_on(request_private_app(
        &owner(),
        &ports,
        &env,
        &private_manifest(&["customer_data:email"]),
    ));
    match bad {
        Err(CommandError::App(AppError::Manifest(e))) => {
            assert_eq!(e.problems[0].path, "scope_reasons");
        }
        other => panic!("{other:?}"),
    }
}
