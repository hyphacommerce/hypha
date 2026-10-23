//! MCP clients connecting agents, on the in-memory repos: the client's metadata
//! document, the person's approval with the agent, the code, tokens bound to the agent and
//! refused elsewhere, rotation, theft detection and revocation.

use std::collections::HashMap;

use hypha_agents::{Agent, AgentClient, AgentKind, Grant, NewAgent, StageAgents};
use hypha_apps::ManifestFetcher;
use hypha_core::{Actor, AgentId, Ctx, HasBatch, Scope, UserId};
use hypha_oauth::{
    AuthorizeError, AuthorizeInput, CodeExchange, CommandError, Refresh, RequestStatus, TokenError,
    UnredirectableError, authenticate_access_token, authenticate_agent_token, authorize_agent,
    deny_agent, exchange_code, get_agent_request, pkce, prepare_agent_approval, refresh,
    revoke_agent_tokens,
};
use hypha_testkit::{InMemoryPorts, TestEnv};
use pollster::block_on;
use uuid::Uuid;

const CLIENT: &str = "https://assistant.example/oauth/client.json";
const REDIRECT: &str = "http://127.0.0.1:41234/callback";
const VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";

struct Documents(HashMap<String, Vec<u8>>);

impl ManifestFetcher for Documents {
    async fn fetch(&self, url: &str) -> Result<Vec<u8>, String> {
        self.0
            .get(url)
            .cloned()
            .ok_or_else(|| "answered HTTP 404".to_string())
    }
}

fn documents() -> Documents {
    let document = serde_json::json!({
        "client_id": CLIENT,
        "client_name": "Assistant",
        "redirect_uris": ["http://127.0.0.1/callback"],
    });
    Documents(HashMap::from([(
        CLIENT.to_string(),
        document.to_string().into_bytes(),
    )]))
}

fn staff() -> Ctx {
    Ctx::new(
        Actor::User(UserId::from_uuid(Uuid::from_u128(1))),
        Scope::ALL.to_vec(),
    )
}

fn input(redirect: &str) -> AuthorizeInput {
    AuthorizeInput {
        response_type: Some("code".to_string()),
        client_id: Some(CLIENT.to_string()),
        redirect_uri: Some(redirect.to_string()),
        scope: Some("orders:read offline_access".to_string()),
        state: Some("s".to_string()),
        code_challenge: Some(pkce::challenge_of(VERIFIER)),
        code_challenge_method: Some("S256".to_string()),
    }
}

fn param(url: &str, name: &str) -> Option<String> {
    url::Url::parse(url)
        .unwrap()
        .query_pairs()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.into_owned())
}

fn agent(n: u128) -> Agent {
    Agent::new(
        AgentId::from_uuid(Uuid::from_u128(n)),
        NewAgent {
            name: "Assistant".to_string(),
            kind: AgentKind::Connected {
                user: UserId::from_uuid(Uuid::from_u128(1)),
            },
            client: Some(AgentClient::new(CLIENT, "Assistant").unwrap()),
            grant: Grant::default(),
        },
    )
    .unwrap()
}

/// Connects a client: the request, the person's approval with a new agent, the code.
fn connect(ports: &InMemoryPorts, env: &TestEnv, n: u128) -> (AgentId, String) {
    let request = block_on(authorize_agent(ports, &documents(), env, input(REDIRECT))).unwrap();
    let request = block_on(get_agent_request(ports, request.id())).unwrap();
    let mut made = agent(n);
    let mut prepared = prepare_agent_approval(
        &staff(),
        ports,
        env,
        request,
        made.id(),
        &[Scope::OrdersRead],
    )
    .unwrap();
    prepared.batch.insert_agent(&mut made);
    block_on(ports.commit(prepared.batch)).unwrap();
    let code = param(&prepared.answer, "code").unwrap();
    (made.id(), code)
}

fn exchange(ports: &InMemoryPorts, env: &TestEnv, code: &str) -> hypha_oauth::IssuedTokens {
    block_on(exchange_code(
        ports,
        env,
        CodeExchange {
            code: code.to_string(),
            code_verifier: VERIFIER.to_string(),
            client_id: CLIENT.to_string(),
            redirect_uri: REDIRECT.to_string(),
        },
    ))
    .unwrap()
}

#[test]
fn a_client_is_checked_against_its_metadata_document() {
    let ports = InMemoryPorts::default();
    let env = TestEnv::default();
    let request = block_on(authorize_agent(&ports, &documents(), &env, input(REDIRECT))).unwrap();
    assert_eq!(request.app(), None);
    assert_eq!(
        request.scopes(),
        [Scope::OrdersRead],
        "offline_access is no Hypha scope"
    );
    assert_eq!(request.agent_client().unwrap().client_name(), "Assistant");
    let elsewhere = block_on(authorize_agent(
        &ports,
        &documents(),
        &env,
        input("https://evil.example/callback"),
    ));
    assert!(matches!(
        elsewhere,
        Err(AuthorizeError::Unredirectable(
            UnredirectableError::RedirectUriNotListed(_)
        ))
    ));
    let mut unknown = input(REDIRECT);
    unknown.client_id = Some("https://nobody.example/client.json".to_string());
    assert!(matches!(
        block_on(authorize_agent(&ports, &documents(), &env, unknown)),
        Err(AuthorizeError::Unredirectable(
            UnredirectableError::ClientMetadata(_)
        ))
    ));
    // Without PKCE it goes back to the client, which is trusted by now.
    let mut plain = input(REDIRECT);
    plain.code_challenge = None;
    assert!(matches!(
        block_on(authorize_agent(&ports, &documents(), &env, plain)),
        Err(AuthorizeError::Redirect {
            error: "invalid_request",
            ..
        })
    ));
}

#[test]
fn only_a_staff_session_connects_a_client() {
    let ports = InMemoryPorts::default();
    let env = TestEnv::default();
    let request = block_on(authorize_agent(&ports, &documents(), &env, input(REDIRECT))).unwrap();
    let key = Ctx::new(
        Actor::ApiKey(hypha_core::ApiKeyId::from_uuid(Uuid::from_u128(5))),
        Scope::ALL.to_vec(),
    );
    assert_eq!(
        prepare_agent_approval(&key, &ports, &env, request.clone(), agent(9).id(), &[]).err(),
        Some(CommandError::SessionRequired)
    );
    let location = block_on(deny_agent(&staff(), &ports, &env, request.id())).unwrap();
    assert_eq!(param(&location, "error").as_deref(), Some("access_denied"));
    let denied = block_on(get_agent_request(&ports, request.id())).unwrap();
    assert_eq!(denied.status(), RequestStatus::Denied);
}

#[test]
fn an_agents_token_works_at_mcp_only_and_rotates() {
    let ports = InMemoryPorts::default();
    let env = TestEnv::default();
    let (agent, code) = connect(&ports, &env, 40);
    let tokens = exchange(&ports, &env, &code);
    assert_eq!(tokens.scopes, [Scope::OrdersRead]);
    assert_eq!(tokens.app_secret, None);
    let access = tokens.access_token.expose().to_string();
    assert_eq!(
        block_on(authenticate_agent_token(&ports, &env, &access)),
        Ok(agent)
    );
    assert_eq!(
        block_on(authenticate_access_token(&ports, &env, &access)).err(),
        Some(CommandError::AgentToken)
    );

    let old = tokens.refresh_token.expose().to_string();
    let renew = |token: &str| Refresh {
        refresh_token: token.to_string(),
        client_id: None,
        scope: None,
    };
    let rotated = block_on(refresh(&ports, &env, renew(&old))).unwrap();
    let new_access = rotated.access_token.expose().to_string();
    assert_eq!(
        block_on(authenticate_agent_token(&ports, &env, &new_access)),
        Ok(agent)
    );
    // The old refresh token again: copied, so every token of the agent goes.
    assert!(matches!(
        block_on(refresh(&ports, &env, renew(&old))),
        Err(TokenError::InvalidGrant(_))
    ));
    assert_eq!(
        block_on(authenticate_agent_token(&ports, &env, &new_access)),
        Err(CommandError::InvalidToken)
    );
}

#[test]
fn an_agents_refresh_token_works_only_for_its_client() {
    let ports = InMemoryPorts::default();
    let env = TestEnv::default();
    let (_, code) = connect(&ports, &env, 42);
    let tokens = exchange(&ports, &env, &code);
    let refresh_token = tokens.refresh_token.expose().to_string();
    let from = |client: &str| Refresh {
        refresh_token: refresh_token.clone(),
        client_id: Some(client.to_string()),
        scope: None,
    };
    assert_eq!(
        block_on(refresh(
            &ports,
            &env,
            from("https://other.example/client.json")
        ))
        .err(),
        Some(TokenError::InvalidGrant(
            "the refresh token was issued to another client".to_string()
        ))
    );
    assert!(block_on(refresh(&ports, &env, from(CLIENT))).is_ok());
}

#[test]
fn revoking_an_agents_tokens_stops_them() {
    let ports = InMemoryPorts::default();
    let env = TestEnv::default();
    let (agent, code) = connect(&ports, &env, 41);
    let tokens = exchange(&ports, &env, &code);
    assert_eq!(block_on(revoke_agent_tokens(&ports, &env, agent)), Ok(1));
    let access = tokens.access_token.expose().to_string();
    assert_eq!(
        block_on(authenticate_agent_token(&ports, &env, &access)),
        Err(CommandError::InvalidToken)
    );
}
