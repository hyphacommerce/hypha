//! OAuth's requests, codes and tokens: round trips, unique hashes, versions, the references
//! (a code's request, a token's app or agent) and revocation; the batch that installs an
//! app; and an MCP client's records, which belong to a connected agent.

use hypha_agents::{
    Agent, AgentClient, AgentKind, AgentRepo, Grant, HasAgents, NewAgent, StageAgents,
};
use hypha_apps::{AppRepo, StageApps};
use hypha_auth::TokenHash;
use hypha_core::{
    Actor, AgentId, AppKey, HasBatch, OAuthRequestId, RepoError, Scope, Timestamp, UserId, Version,
    Versioned,
};
use hypha_oauth::{
    AuthorizationCode, AuthorizationRequest, ClientMetadata, NewCode, NewRequest, NewTokenPair,
    OAuthClient, OAuthCodeRepo, OAuthPorts, OAuthRequestRepo, OAuthToken, OAuthTokenRepo,
    RequestClient, RequestStatus, StageOAuth, keys,
};
use uuid::Uuid;

use super::{apps::installed_app, scenario};

fn at(seconds: i64) -> Timestamp {
    Timestamp::from_unix_micros((1_800_000_000 + seconds) * 1_000_000)
}

fn key(k: &str) -> AppKey {
    AppKey::new(k).unwrap()
}

fn request(n: u128, app: &str, created: i64) -> AuthorizationRequest {
    AuthorizationRequest::new(NewRequest {
        id: OAuthRequestId::from_uuid(Uuid::from_u128(n)),
        client_id: "https://app.example/hypha-app.json".to_string(),
        client: RequestClient::App(Box::new(crate::app_manifest(app, "https://app.example"))),
        redirect_uri: "https://app.example/callback".to_string(),
        state: Some("xyz".to_string()),
        code_challenge: Some("E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM".to_string()),
        scopes: vec![Scope::OrdersRead, Scope::CustomerDataEmail],
        previous_scopes: Some(vec![Scope::OrdersRead]),
        created_at: at(created),
        expires_at: at(created + 600),
    })
}

fn code(n: u8, request: OAuthRequestId, app: &str) -> AuthorizationCode {
    AuthorizationCode::new(NewCode {
        hash: TokenHash::from_bytes([n; 32]),
        request,
        client: OAuthClient::App(key(app)),
        client_id: "https://app.example/hypha-app.json".to_string(),
        redirect_uri: "https://app.example/callback".to_string(),
        code_challenge: "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM".to_string(),
        scopes: vec![Scope::OrdersRead],
        first_install: true,
        family: Uuid::from_u128(u128::from(n) + 500),
        expires_at: at(60),
    })
}

fn token(n: u8, family: u128, app: &str) -> OAuthToken {
    OAuthToken::new(NewTokenPair {
        id: Uuid::from_u128(u128::from(n)),
        family: Uuid::from_u128(family),
        client: OAuthClient::App(key(app)),
        scopes: vec![Scope::OrdersRead, Scope::CustomerDataEmail],
        access_hash: TokenHash::from_bytes([n; 32]),
        access_expires_at: at(3600),
        refresh_hash: TokenHash::from_bytes([n.wrapping_add(100); 32]),
        refresh_expires_at: at(86_400),
        created_at: at(0),
    })
}

const MCP_CLIENT: &str = "https://client.example/oauth/metadata.json";

/// An MCP client's request: no app, its metadata document instead.
fn agent_request(n: u128) -> AuthorizationRequest {
    let document = serde_json::json!({
        "client_id": MCP_CLIENT,
        "client_name": "Example client",
        "redirect_uris": ["http://127.0.0.1/callback"],
        "logo_uri": "https://client.example/logo.png",
    });
    let metadata = ClientMetadata::parse(MCP_CLIENT, document.to_string().as_bytes()).unwrap();
    AuthorizationRequest::new(NewRequest {
        id: OAuthRequestId::from_uuid(Uuid::from_u128(n)),
        client_id: MCP_CLIENT.to_string(),
        client: RequestClient::Agent(metadata),
        redirect_uri: "http://127.0.0.1:5000/callback".to_string(),
        state: None,
        code_challenge: Some("E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM".to_string()),
        scopes: vec![Scope::OrdersRead],
        previous_scopes: None,
        created_at: at(0),
        expires_at: at(600),
    })
}

fn connected_agent(n: u128) -> Agent {
    Agent::new(
        AgentId::from_uuid(Uuid::from_u128(n)),
        NewAgent {
            name: "Claude".to_string(),
            kind: AgentKind::Connected {
                user: UserId::from_uuid(Uuid::from_u128(3)),
            },
            client: Some(AgentClient::new(MCP_CLIENT, "Example client").unwrap()),
            grant: Grant::default(),
        },
    )
    .unwrap()
}

/// Runs every OAuth repo scenario, each on a fresh store from `make`.
pub async fn oauth_repos<S: OAuthPorts + HasAgents + HasBatch<Batch: StageAgents>>(
    make: impl AsyncFn() -> S,
) {
    scenario("oauth: a request round trips; versions; an app's history newest first");
    requests(&make().await).await;
    scenario("oauth: a code needs its request, is unique, and is spent once");
    codes(&make().await).await;
    scenario("oauth: tokens need their app, hashes are unique, families and apps revoke");
    tokens(&make().await).await;
    scenario("oauth: installing writes the app, the request and the code at once, or none");
    install_batch(&make().await).await;
    scenario("oauth: an MCP client's request, code and tokens belong to its agent");
    agent_client(&make().await).await;
}

async fn agent_client<S: OAuthPorts + HasAgents + HasBatch<Batch: StageAgents>>(store: &S) {
    let mut req = agent_request(1);
    store.oauth_requests().insert(&mut req).await.unwrap();
    assert_eq!(
        store.oauth_requests().get(req.id()).await,
        Ok(Some(req.clone()))
    );
    assert_eq!(req.app(), None);

    // Connecting writes the agent, the decided request and the code at once.
    let staff = Actor::User(UserId::from_uuid(Uuid::from_u128(3)));
    req.decide(true, staff, at(5)).unwrap();
    let mut agent = connected_agent(40);
    let agent_id = agent.id();
    let mut code = AuthorizationCode::new(NewCode {
        hash: TokenHash::from_bytes([9; 32]),
        request: req.id(),
        client: OAuthClient::Agent(agent_id),
        client_id: MCP_CLIENT.to_string(),
        redirect_uri: "http://127.0.0.1:5000/callback".to_string(),
        code_challenge: "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM".to_string(),
        scopes: vec![Scope::OrdersRead],
        first_install: false,
        family: Uuid::from_u128(600),
        expires_at: at(60),
    });
    let mut batch = store.batch();
    batch.insert_agent(&mut agent);
    batch.update_oauth_request(&mut req);
    batch.insert_oauth_code(&mut code);
    store.commit(batch).await.unwrap();
    assert_eq!(
        store.oauth_codes().get(code.hash()).await,
        Ok(Some(code.clone()))
    );
    assert!(store.agents().get(agent_id).await.unwrap().is_some());

    let agent_token = |n: u8, family: u128, agent: AgentId| {
        OAuthToken::new(NewTokenPair {
            id: Uuid::from_u128(u128::from(n)),
            family: Uuid::from_u128(family),
            client: OAuthClient::Agent(agent),
            scopes: vec![Scope::OrdersRead],
            access_hash: TokenHash::from_bytes([n; 32]),
            access_expires_at: at(3600),
            refresh_hash: TokenHash::from_bytes([n.wrapping_add(100); 32]),
            refresh_expires_at: at(86_400),
            created_at: at(0),
        })
    };
    // A token needs its agent.
    let mut batch = store.batch();
    batch.insert_oauth_token(&mut agent_token(
        1,
        1,
        AgentId::from_uuid(Uuid::from_u128(99)),
    ));
    assert!(matches!(
        store.commit(batch).await,
        Err(RepoError::Backend(_) | RepoError::Reference(_))
    ));

    let repo = store.oauth_tokens();
    let mut first = agent_token(2, 1, agent_id);
    repo.insert(&mut first).await.unwrap();
    assert_eq!(
        repo.get_by_access(first.access_hash()).await,
        Ok(Some(first.clone()))
    );
    let mut other_family = agent_token(3, 2, agent_id);
    repo.insert(&mut other_family).await.unwrap();
    // An app's tokens are another client's: revoking the agent's leaves them.
    store
        .apps()
        .insert(&mut installed_app("app_shop", &[Scope::OrdersRead]))
        .await
        .unwrap();
    let mut app_pair = token(4, 3, "app_shop");
    repo.insert(&mut app_pair).await.unwrap();
    assert_eq!(repo.revoke_app(&key("app_shop"), at(10)).await, Ok(1));
    assert_eq!(repo.revoke_agent(agent_id, at(20)).await, Ok(2));
    assert_eq!(repo.revoke_agent(agent_id, at(21)).await, Ok(0));
    let revoked = repo
        .get_by_refresh(other_family.refresh_hash())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(revoked.revoked_at(), Some(at(20)));
    assert_eq!(revoked.client(), &OAuthClient::Agent(agent_id));
}

async fn requests<S: OAuthPorts>(store: &S) {
    let repo = store.oauth_requests();
    let mut first = request(1, "app_shop", 0);
    assert_eq!(repo.get(first.id()).await, Ok(None));
    repo.insert(&mut first).await.unwrap();
    assert_eq!(first.version(), Version::new(1));
    assert_eq!(repo.get(first.id()).await, Ok(Some(first.clone())));
    assert_eq!(
        repo.insert(&mut request(1, "app_shop", 0)).await,
        Err(RepoError::Duplicate(keys::OAUTH_REQUEST_ID))
    );

    let stale = first.clone();
    let staff = Actor::User(UserId::from_uuid(Uuid::from_u128(3)));
    first.decide(true, staff, at(5)).unwrap();
    repo.update(&mut first).await.unwrap();
    assert_eq!(first.version(), Version::new(2));
    let stored = repo.get(first.id()).await.unwrap().unwrap();
    assert_eq!(stored, first);
    assert_eq!(stored.status(), RequestStatus::Approved);
    assert_eq!(stored.decided_by(), Some(staff));
    assert_eq!(
        repo.update(&mut stale.clone()).await,
        Err(RepoError::StaleVersion)
    );

    // A request a manifest refresh started has no PKCE challenge, and keeps none.
    let mut second = request(2, "app_shop", 10);
    second = AuthorizationRequest::restore(hypha_oauth::RequestSnapshot {
        code_challenge: None,
        state: None,
        ..second.snapshot()
    })
    .unwrap();
    repo.insert(&mut second).await.unwrap();
    assert_eq!(repo.get(second.id()).await, Ok(Some(second.clone())));
    repo.insert(&mut request(3, "app_other", 20)).await.unwrap();
    assert_eq!(
        repo.list_for_app(&key("app_shop"), 10).await,
        Ok(vec![second.clone(), first])
    );
    assert_eq!(
        repo.list_for_app(&key("app_shop"), 1).await,
        Ok(vec![second])
    );
}

async fn codes<S: OAuthPorts>(store: &S) {
    let repo = store.oauth_codes();
    let orphan = code(
        1,
        OAuthRequestId::from_uuid(Uuid::from_u128(99)),
        "app_shop",
    );
    assert!(matches!(
        repo.insert(&mut orphan.clone()).await,
        Err(RepoError::Backend(_) | RepoError::Reference(_))
    ));
    let mut req = request(1, "app_shop", 0);
    store.oauth_requests().insert(&mut req).await.unwrap();
    let mut first = code(1, req.id(), "app_shop");
    repo.insert(&mut first).await.unwrap();
    assert_eq!(repo.get(first.hash()).await, Ok(Some(first.clone())));
    assert_eq!(repo.get(TokenHash::from_bytes([9; 32])).await, Ok(None));
    assert_eq!(
        repo.insert(&mut code(1, req.id(), "app_shop")).await,
        Err(RepoError::Duplicate(keys::OAUTH_CODE))
    );

    let racing = first.clone();
    assert!(first.use_up(at(10)));
    repo.update(&mut first).await.unwrap();
    assert_eq!(repo.get(first.hash()).await, Ok(Some(first.clone())));
    let mut loser = racing;
    assert!(loser.use_up(at(11)));
    assert_eq!(repo.update(&mut loser).await, Err(RepoError::StaleVersion));
}

async fn tokens<S: OAuthPorts>(store: &S) {
    let repo = store.oauth_tokens();
    assert!(matches!(
        repo.insert(&mut token(1, 1, "app_shop")).await,
        Err(RepoError::Backend(_) | RepoError::Reference(_))
    ));
    for k in ["app_shop", "app_other"] {
        store
            .apps()
            .insert(&mut installed_app(k, &[Scope::OrdersRead]))
            .await
            .unwrap();
    }
    let mut first = token(1, 1, "app_shop");
    repo.insert(&mut first).await.unwrap();
    assert_eq!(first.version(), Version::new(1));
    assert_eq!(
        repo.get_by_access(first.access_hash()).await,
        Ok(Some(first.clone()))
    );
    assert_eq!(
        repo.get_by_refresh(first.refresh_hash()).await,
        Ok(Some(first.clone()))
    );
    assert_eq!(repo.get_by_access(first.refresh_hash()).await, Ok(None));

    let mut same_access = token(2, 1, "app_shop");
    let mut s = same_access.snapshot();
    s.access_hash = first.access_hash();
    same_access = OAuthToken::restore(s).unwrap();
    assert_eq!(
        repo.insert(&mut same_access).await,
        Err(RepoError::Duplicate(keys::OAUTH_ACCESS_TOKEN))
    );
    let mut same_refresh = token(2, 1, "app_shop");
    let mut s = same_refresh.snapshot();
    s.refresh_hash = first.refresh_hash();
    same_refresh = OAuthToken::restore(s).unwrap();
    assert_eq!(
        repo.insert(&mut same_refresh).await,
        Err(RepoError::Duplicate(keys::OAUTH_REFRESH_TOKEN))
    );

    first.rotate(at(30));
    repo.update(&mut first).await.unwrap();
    assert_eq!(
        repo.get_by_refresh(first.refresh_hash()).await,
        Ok(Some(first.clone()))
    );
    let mut second = token(2, 1, "app_shop");
    repo.insert(&mut second).await.unwrap();
    let mut other_family = token(3, 2, "app_shop");
    repo.insert(&mut other_family).await.unwrap();
    let mut other_app = token(4, 3, "app_other");
    repo.insert(&mut other_app).await.unwrap();

    assert_eq!(repo.revoke_family(Uuid::from_u128(1), at(40)).await, Ok(2));
    assert_eq!(repo.revoke_family(Uuid::from_u128(1), at(41)).await, Ok(0));
    let revoked = repo
        .get_by_access(second.access_hash())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(revoked.revoked_at(), Some(at(40)));
    assert_eq!(revoked.version(), Version::new(2));
    assert_eq!(
        repo.update(&mut second).await,
        Err(RepoError::StaleVersion),
        "revoking bumped the version"
    );
    assert_eq!(repo.revoke_app(&key("app_shop"), at(50)).await, Ok(1));
    assert_eq!(
        repo.get_by_access(other_app.access_hash())
            .await
            .unwrap()
            .unwrap()
            .revoked_at(),
        None
    );

    store.apps().delete(&key("app_other")).await.unwrap();
    assert_eq!(repo.get_by_access(other_app.access_hash()).await, Ok(None));
}

async fn install_batch<S: OAuthPorts>(store: &S) {
    let mut req = request(1, "app_shop", 0);
    store.oauth_requests().insert(&mut req).await.unwrap();
    let staff = Actor::User(UserId::from_uuid(Uuid::from_u128(3)));
    req.decide(true, staff, at(5)).unwrap();
    let mut app = installed_app("app_shop", &[Scope::OrdersRead]);
    let mut first_code = code(1, req.id(), "app_shop");
    let mut batch = store.batch();
    batch.insert_app(&mut app);
    batch.update_oauth_request(&mut req);
    batch.insert_oauth_code(&mut first_code);
    store.commit(batch).await.unwrap();
    assert_eq!(app.version(), Version::new(1));
    assert_eq!(
        store.apps().get(&key("app_shop")).await,
        Ok(Some(app.clone()))
    );
    assert_eq!(
        store.oauth_requests().get(req.id()).await,
        Ok(Some(req.clone()))
    );
    assert_eq!(
        store.oauth_codes().get(first_code.hash()).await,
        Ok(Some(first_code.clone()))
    );

    // A second install of the same key loses: nothing of its batch is written.
    let mut again = request(2, "app_shop", 1);
    store.oauth_requests().insert(&mut again).await.unwrap();
    let before = again.clone();
    again.decide(true, staff, at(6)).unwrap();
    let mut twin = installed_app("app_shop", &[Scope::OrdersRead]);
    let mut second_code = code(2, again.id(), "app_shop");
    let mut batch = store.batch();
    batch.insert_app(&mut twin);
    batch.update_oauth_request(&mut again);
    batch.insert_oauth_code(&mut second_code);
    assert!(matches!(
        store.commit(batch).await,
        Err(RepoError::Duplicate(
            hypha_apps::keys::APP_KEY | hypha_apps::keys::APP_ID
        ))
    ));
    assert_eq!(
        store.oauth_requests().get(again.id()).await,
        Ok(Some(before))
    );
    assert_eq!(store.oauth_codes().get(second_code.hash()).await, Ok(None));

    // A token and the app's revocation in one batch: the revocation comes after.
    let mut pair = token(7, 9, "app_shop");
    let mut batch = store.batch();
    batch.insert_oauth_token(&mut pair);
    batch.revoke_app_tokens(&key("app_shop"), at(70));
    store.commit(batch).await.unwrap();
    let stored = store
        .oauth_tokens()
        .get_by_access(pair.access_hash())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.revoked_at(), Some(at(70)));
}
