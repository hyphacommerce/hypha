//! What every agents store must do: agents (with the use they record apart from their
//! version), proposals (one per agent and idempotency key, listed by filter, found when
//! due) and the call log, written directly and staged in a batch.

use hypha_agents::{
    Agent, AgentCall, AgentCallRepo, AgentClient, AgentFilter, AgentKind, AgentRepo, CallMode,
    Grant, GrantInput, HasAgentCalls, HasAgents, HasProposals, NewAgent, NewAgentCall, NewProposal,
    OperationId, Proposal, ProposalFilter, ProposalRepo, ProposalRequest, ProposalResponse,
    ProposalStatus, StageAgents, keys,
};
use hypha_core::{
    AgentCallId, AgentId, HasBatch, IdempotencyKey, PageLimit, PageRequest, ProposalId, RepoError,
    Scope, Stamped, Timestamp, UserId, Version, Versioned,
};
use serde_json::{Map, json};
use uuid::Uuid;

use super::scenario;

fn aid(n: u128) -> AgentId {
    AgentId::from_uuid(Uuid::from_u128(n))
}

fn pid(n: u128) -> ProposalId {
    ProposalId::from_uuid(Uuid::from_u128(n))
}

fn cid(n: u128) -> AgentCallId {
    AgentCallId::from_uuid(Uuid::from_u128(n))
}

fn user(n: u128) -> UserId {
    UserId::from_uuid(Uuid::from_u128(500 + n))
}

/// A time well after the epoch, `n` seconds on.
fn at(n: i64) -> Timestamp {
    Timestamp::from_unix_micros(1_800_000_000_000_000 + n * 1_000_000)
}

fn full_grant() -> Grant {
    Grant::new(GrantInput {
        read: vec!["orders:read".to_string(), "agents:read".to_string()],
        write: vec![
            ("products:write".to_string(), "propose".to_string()),
            ("inventory:write".to_string(), "apply".to_string()),
            ("customers:erase".to_string(), "propose".to_string()),
        ],
        customer_data: vec![
            "customer_data:email".to_string(),
            "customer_data:address".to_string(),
        ],
        operations: Some(vec![
            "admin.products.update".to_string(),
            "app.app_reviews.import-csv".to_string(),
        ]),
    })
    .unwrap()
}

fn autonomous(n: u128, name: &str) -> Agent {
    Agent::new(
        aid(n),
        NewAgent {
            name: name.to_string(),
            kind: AgentKind::Autonomous,
            client: None,
            grant: Grant::default(),
        },
    )
    .unwrap()
}

fn connected(n: u128, u: u128) -> Agent {
    Agent::new(
        aid(n),
        NewAgent {
            name: "Claude".to_string(),
            kind: AgentKind::Connected { user: user(u) },
            client: Some(
                AgentClient::new("https://claude.ai/oauth/mcp-client.json", "Claude").unwrap(),
            ),
            grant: full_grant(),
        },
    )
    .unwrap()
}

fn page<Id>(after: Option<Id>, limit: u32) -> PageRequest<Id> {
    PageRequest {
        after,
        limit: PageLimit::new(limit).unwrap(),
    }
}

/// Runs every `AgentRepo` scenario, each on fresh ports from `make`.
pub async fn agent_repo<S>(make: impl AsyncFn() -> S)
where
    S: HasAgents + HasBatch<Batch: StageAgents>,
{
    scenario("agent: every field round-trips at version 1, both kinds");
    agent_round_trip(&make().await).await;
    scenario("agent: an ID is stored once; updates bump the version, stale ones fail");
    agent_versions(&make().await).await;
    scenario("agent: use is recorded apart, never backwards, and updates keep it");
    agent_use(&make().await).await;
    scenario("agent: lists page newest first, by person, revoked only when asked");
    agent_lists(&make().await).await;
    scenario("agent: agents staged in a batch commit together, or not at all");
    agent_batch(&make().await).await;
}

async fn agent_round_trip<S: HasAgents>(ports: &S) {
    let repo = ports.agents();
    let mut bot = autonomous(1, "Restocker");
    let mut claude = connected(2, 1);
    repo.insert(&mut bot).await.unwrap();
    repo.insert(&mut claude).await.unwrap();
    assert_eq!(claude.version(), Version::new(1));
    let stored = repo.get(aid(2)).await.unwrap().unwrap();
    assert_eq!(stored, claude);
    assert!(stored.stamps().is_some());
    assert_eq!(repo.get(aid(1)).await, Ok(Some(bot)));
    assert_eq!(repo.get(aid(9)).await, Ok(None));
}

async fn agent_versions<S: HasAgents>(ports: &S) {
    let repo = ports.agents();
    let mut agent = autonomous(1, "Restocker");
    repo.insert(&mut agent).await.unwrap();
    assert_eq!(
        repo.insert(&mut autonomous(1, "Other")).await,
        Err(RepoError::Duplicate(keys::AGENT_ID))
    );
    let mut stale = agent.clone();
    agent.rename("Night restocker").unwrap();
    agent.set_grant(full_grant()).unwrap();
    agent.set_enabled(false).unwrap();
    repo.update(&mut agent).await.unwrap();
    assert_eq!(agent.version(), Version::new(2));
    assert_eq!(repo.get(aid(1)).await, Ok(Some(agent.clone())));
    stale.rename("Lost").unwrap();
    assert_eq!(repo.update(&mut stale).await, Err(RepoError::StaleVersion));
    agent.revoke(at(5)).unwrap();
    repo.update(&mut agent).await.unwrap();
    assert_eq!(repo.get(aid(1)).await, Ok(Some(agent)));
    let mut missing = autonomous(9, "Ghost");
    missing.set_version(Version::new(1));
    assert_eq!(
        repo.update(&mut missing).await,
        Err(RepoError::StaleVersion)
    );
}

async fn agent_use<S: HasAgents>(ports: &S) {
    let repo = ports.agents();
    let mut agent = autonomous(1, "Restocker");
    repo.insert(&mut agent).await.unwrap();
    repo.record_use(aid(1), at(10)).await.unwrap();
    let used = repo.get(aid(1)).await.unwrap().unwrap();
    assert_eq!(used.last_used_at(), Some(at(10)));
    assert_eq!(used.version(), Version::new(1), "use bumps no version");
    repo.record_use(aid(1), at(5)).await.unwrap();
    let kept = repo.get(aid(1)).await.unwrap().unwrap();
    assert_eq!(kept.last_used_at(), Some(at(10)), "never backwards");
    // `agent` was loaded before the use: updating it keeps the stored use.
    agent.rename("Renamed").unwrap();
    repo.update(&mut agent).await.unwrap();
    let renamed = repo.get(aid(1)).await.unwrap().unwrap();
    assert_eq!(renamed.name(), "Renamed");
    assert_eq!(renamed.last_used_at(), Some(at(10)));
    repo.record_use(aid(9), at(10)).await.unwrap();
}

async fn agent_lists<S: HasAgents>(ports: &S) {
    let repo = ports.agents();
    repo.insert(&mut connected(1, 1)).await.unwrap();
    repo.insert(&mut autonomous(2, "Two")).await.unwrap();
    repo.insert(&mut connected(3, 2)).await.unwrap();
    let mut revoked = connected(4, 1);
    revoked.revoke(at(1)).unwrap();
    repo.insert(&mut revoked).await.unwrap();
    let ids = async |filter: AgentFilter, after: Option<u128>, limit: u32| {
        let found = repo
            .list(&filter, page(after.map(aid), limit))
            .await
            .unwrap();
        let ids: Vec<AgentId> = found.items.iter().map(Agent::id).collect();
        (ids, found.has_more)
    };
    assert_eq!(
        ids(AgentFilter::default(), None, 2).await,
        (vec![aid(3), aid(2)], true)
    );
    assert_eq!(
        ids(AgentFilter::default(), Some(2), 2).await,
        (vec![aid(1)], false)
    );
    let mine = AgentFilter {
        user: Some(user(1)),
        include_revoked: false,
    };
    assert_eq!(ids(mine.clone(), None, 10).await, (vec![aid(1)], false));
    let all_mine = AgentFilter {
        include_revoked: true,
        ..mine
    };
    assert_eq!(ids(all_mine, None, 10).await, (vec![aid(4), aid(1)], false));
}

async fn agent_batch<S: HasAgents + HasBatch<Batch: StageAgents>>(ports: &S) {
    let mut one = autonomous(1, "One");
    let mut two = autonomous(2, "Two");
    let mut batch = ports.batch();
    batch.insert_agent(&mut one);
    batch.insert_agent(&mut two);
    ports.commit(batch).await.unwrap();
    assert_eq!(one.version(), Version::new(1));
    assert_eq!(ports.agents().get(aid(2)).await, Ok(Some(two.clone())));

    // A failing insert fails the whole batch: the new agent staged with it isn't written.
    let mut three = autonomous(3, "Three");
    let mut again = autonomous(1, "One again");
    let mut batch = ports.batch();
    batch.insert_agent(&mut three);
    batch.insert_agent(&mut again);
    assert_eq!(
        ports.commit(batch).await,
        Err(RepoError::Duplicate(keys::AGENT_ID))
    );
    assert_eq!(ports.agents().get(aid(3)).await, Ok(None));
}

fn request() -> ProposalRequest {
    let mut params = Map::new();
    params.insert(
        "id".to_string(),
        json!("prod_0190f1a2b3c47d8e9f00112233445566"),
    );
    let mut query = Map::new();
    query.insert("notify".to_string(), json!(true));
    query.insert("limit".to_string(), json!(20));
    ProposalRequest {
        params,
        query,
        body: Some(json!({
            "title": "Mug",
            "tags": ["kitchen", "gift"],
            "weight": null,
            "nested": {"z": 1, "a": [1, 2, {"b": false}]}
        })),
    }
}

fn proposal(n: u128, agent: u128, key: Option<&str>, expires: i64) -> Proposal {
    Proposal::new(
        pid(n),
        NewProposal {
            agent: aid(agent),
            operation: OperationId::new("admin.products.update").unwrap(),
            request: request(),
            idempotency_key: key.map(|k| IdempotencyKey::new(k).unwrap()),
            preview: json!({"complete": true, "events": [{"type": "product.updated"}]}),
            digest: "sha256:9f86d081884c7d65".to_string(),
            scopes: vec![Scope::ProductsWrite, Scope::InventoryWrite],
            expires_at: at(expires),
        },
    )
    .unwrap()
}

fn needing(n: u128, scopes: Vec<Scope>) -> Proposal {
    Proposal::new(
        pid(n),
        NewProposal {
            scopes,
            ..proposal_parts(n)
        },
    )
    .unwrap()
}

fn proposal_parts(n: u128) -> NewProposal {
    let p = proposal(n, 1, None, 100);
    let s = p.snapshot();
    NewProposal {
        agent: s.agent,
        operation: s.operation,
        request: s.request,
        idempotency_key: s.idempotency_key,
        preview: s.preview,
        digest: s.digest,
        scopes: s.scopes,
        expires_at: s.expires_at,
    }
}

fn response() -> ProposalResponse {
    ProposalResponse::new(200, json!({"id": "prod_1", "price": {"amount": 2490}})).unwrap()
}

/// Runs every `ProposalRepo` scenario, each on fresh ports from `make`.
pub async fn proposal_repo<S>(make: impl AsyncFn() -> S)
where
    S: HasAgents + HasProposals,
{
    scenario("proposal: every field round-trips, pending, applying and in every decided state");
    proposal_round_trip(&make().await).await;
    scenario("proposal: its agent must exist");
    proposal_needs_agent(&make().await).await;
    scenario("proposal: one per agent and idempotency key, found by it");
    proposal_keys(&make().await).await;
    scenario("proposal: updates bump the version, stale ones fail");
    proposal_versions(&make().await).await;
    scenario("proposal: lists page newest first, by agent, status and scopes held");
    proposal_lists(&make().await).await;
    scenario("proposal: due ones are pending or applying past their lifetime, earliest first");
    proposal_due(&make().await).await;
}

async fn with_agents<S: HasAgents>(ports: &S, agents: &[u128]) {
    for n in agents {
        ports
            .agents()
            .insert(&mut autonomous(*n, "Agent"))
            .await
            .unwrap();
    }
}

async fn proposal_round_trip<S: HasAgents + HasProposals>(ports: &S) {
    with_agents(ports, &[1]).await;
    let repo = ports.proposals();
    let mut pending = proposal(1, 1, Some("k-1"), 100);
    repo.insert(&mut pending).await.unwrap();
    assert_eq!(pending.version(), Version::new(1));
    let stored = repo.get(pid(1)).await.unwrap().unwrap();
    assert_eq!(stored, pending);
    assert!(stored.stamps().is_some());
    assert_eq!(repo.get(pid(9)).await, Ok(None));

    let mut bodiless = Proposal::new(
        pid(2),
        NewProposal {
            request: ProposalRequest::default(),
            idempotency_key: None,
            ..proposal_parts(2)
        },
    )
    .unwrap();
    repo.insert(&mut bodiless).await.unwrap();
    assert_eq!(repo.get(pid(2)).await, Ok(Some(bodiless)));

    // Previewed again on a stale approval: a new preview and digest, pending again.
    let mut refreshed = proposal(7, 1, None, 100);
    repo.insert(&mut refreshed).await.unwrap();
    refreshed.start_applying(user(1), at(2)).unwrap();
    refreshed
        .refresh(
            user(1),
            serde_json::json!({"complete": true, "ids": ["x"]}),
            "sha256:fresh".to_string(),
        )
        .unwrap();
    repo.update(&mut refreshed).await.unwrap();
    let stored = repo.get(pid(7)).await.unwrap().unwrap();
    assert_eq!((stored.refreshes(), stored.digest()), (1, "sha256:fresh"));
    assert_eq!(stored, refreshed);

    type Decide = fn(&mut Proposal);
    let decisions: [(u128, Decide); 5] = [
        (3, |p| {
            p.start_applying(user(1), at(4)).unwrap();
            p.applied(user(1), at(5), response()).unwrap()
        }),
        (4, |p| {
            p.start_applying(user(1), at(4)).unwrap();
            p.failed(user(1), at(5), response()).unwrap()
        }),
        (8, |p| p.start_applying(user(1), at(4)).unwrap()),
        (5, |p| {
            p.reject(user(2), at(5), Some("Not now".to_string()))
                .unwrap()
        }),
        (6, |p| p.expire(at(100)).unwrap()),
    ];
    for (n, decide) in decisions {
        let mut p = proposal(n, 1, None, 100);
        repo.insert(&mut p).await.unwrap();
        decide(&mut p);
        repo.update(&mut p).await.unwrap();
        assert_eq!(repo.get(pid(n)).await, Ok(Some(p)));
    }
}

async fn proposal_needs_agent<S: HasAgents + HasProposals>(ports: &S) {
    let result = ports
        .proposals()
        .insert(&mut proposal(1, 7, None, 100))
        .await;
    assert!(matches!(result, Err(RepoError::Backend(_))), "{result:?}");
    assert_eq!(ports.proposals().get(pid(1)).await, Ok(None));
}

async fn proposal_keys<S: HasAgents + HasProposals>(ports: &S) {
    with_agents(ports, &[1, 2]).await;
    let repo = ports.proposals();
    let mut first = proposal(1, 1, Some("same"), 100);
    repo.insert(&mut first).await.unwrap();
    assert_eq!(
        repo.insert(&mut proposal(2, 1, Some("same"), 100)).await,
        Err(RepoError::Duplicate(keys::PROPOSAL_KEY))
    );
    assert_eq!(
        repo.insert(&mut proposal(1, 2, None, 100)).await,
        Err(RepoError::Duplicate(keys::PROPOSAL_ID))
    );
    // Another agent's key, and proposals without one, don't clash.
    repo.insert(&mut proposal(3, 2, Some("same"), 100))
        .await
        .unwrap();
    repo.insert(&mut proposal(4, 1, None, 100)).await.unwrap();
    repo.insert(&mut proposal(5, 1, None, 100)).await.unwrap();
    let key = IdempotencyKey::new("same").unwrap();
    assert_eq!(repo.find_by_key(aid(1), &key).await, Ok(Some(first)));
    assert_eq!(
        repo.find_by_key(aid(2), &key)
            .await
            .unwrap()
            .map(|p| p.id()),
        Some(pid(3))
    );
    let other = IdempotencyKey::new("other").unwrap();
    assert_eq!(repo.find_by_key(aid(1), &other).await, Ok(None));
}

async fn proposal_versions<S: HasAgents + HasProposals>(ports: &S) {
    with_agents(ports, &[1]).await;
    let repo = ports.proposals();
    let mut p = proposal(1, 1, None, 100);
    repo.insert(&mut p).await.unwrap();
    let mut stale = p.clone();
    p.reject(user(1), at(1), None).unwrap();
    repo.update(&mut p).await.unwrap();
    assert_eq!(p.version(), Version::new(2));
    stale.start_applying(user(1), at(2)).unwrap();
    assert_eq!(repo.update(&mut stale).await, Err(RepoError::StaleVersion));
    assert_eq!(
        repo.get(pid(1)).await.unwrap().map(|p| p.status()),
        Some(ProposalStatus::Rejected)
    );
}

async fn proposal_lists<S: HasAgents + HasProposals>(ports: &S) {
    with_agents(ports, &[1, 2]).await;
    let repo = ports.proposals();
    repo.insert(&mut needing(1, vec![Scope::ProductsWrite]))
        .await
        .unwrap();
    repo.insert(&mut needing(
        2,
        vec![Scope::OrdersWrite, Scope::InventoryWrite],
    ))
    .await
    .unwrap();
    let mut decided = needing(3, vec![Scope::ProductsWrite]);
    repo.insert(&mut decided).await.unwrap();
    decided.reject(user(1), at(1), None).unwrap();
    repo.update(&mut decided).await.unwrap();
    repo.insert(&mut proposal(4, 2, None, 100)).await.unwrap();
    repo.insert(&mut needing(5, vec![])).await.unwrap();

    let ids = async |filter: ProposalFilter, after: Option<u128>, limit: u32| {
        let found = repo
            .list(&filter, page(after.map(pid), limit))
            .await
            .unwrap();
        let ids: Vec<ProposalId> = found.items.iter().map(Proposal::id).collect();
        (ids, found.has_more)
    };
    assert_eq!(
        ids(ProposalFilter::default(), None, 2).await,
        (vec![pid(5), pid(4)], true)
    );
    assert_eq!(
        ids(ProposalFilter::default(), Some(4), 10).await,
        (vec![pid(3), pid(2), pid(1)], false)
    );
    let of_two = ProposalFilter {
        agent: Some(aid(2)),
        ..ProposalFilter::default()
    };
    assert_eq!(ids(of_two, None, 10).await, (vec![pid(4)], false));
    let rejected = ProposalFilter {
        status: Some(ProposalStatus::Rejected),
        ..ProposalFilter::default()
    };
    assert_eq!(ids(rejected, None, 10).await, (vec![pid(3)], false));
    // Every scope must be held; one needing none is approvable by anyone.
    let within = ProposalFilter {
        within: Some(vec![
            Scope::ProductsRead,
            Scope::ProductsWrite,
            Scope::InventoryWrite,
        ]),
        status: Some(ProposalStatus::Pending),
        ..ProposalFilter::default()
    };
    assert_eq!(
        ids(within, None, 10).await,
        (vec![pid(5), pid(4), pid(1)], false)
    );
    let nothing = ProposalFilter {
        within: Some(vec![]),
        ..ProposalFilter::default()
    };
    assert_eq!(ids(nothing, None, 10).await, (vec![pid(5)], false));
}

async fn proposal_due<S: HasAgents + HasProposals>(ports: &S) {
    with_agents(ports, &[1]).await;
    let repo = ports.proposals();
    repo.insert(&mut proposal(1, 1, None, 30)).await.unwrap();
    repo.insert(&mut proposal(2, 1, None, 10)).await.unwrap();
    repo.insert(&mut proposal(3, 1, None, 20)).await.unwrap();
    repo.insert(&mut proposal(4, 1, None, 21)).await.unwrap();
    let mut decided = proposal(5, 1, None, 5);
    repo.insert(&mut decided).await.unwrap();
    decided.reject(user(1), at(1), None).unwrap();
    repo.update(&mut decided).await.unwrap();
    // An approval's write running: due too (the job decides whether its lease passed).
    let mut applying = proposal(6, 1, None, 15);
    repo.insert(&mut applying).await.unwrap();
    applying.start_applying(user(1), at(1)).unwrap();
    repo.update(&mut applying).await.unwrap();
    let due = async |now: i64, limit: u32| -> Vec<ProposalId> {
        repo.due(at(now), limit)
            .await
            .unwrap()
            .iter()
            .map(Proposal::id)
            .collect()
    };
    assert_eq!(due(9, 10).await, Vec::<ProposalId>::new());
    assert_eq!(
        due(20, 10).await,
        vec![pid(2), pid(6), pid(3)],
        "at its end is due"
    );
    assert_eq!(due(100, 2).await, vec![pid(2), pid(6)]);
    assert_eq!(
        due(100, 10).await,
        vec![pid(2), pid(6), pid(3), pid(4), pid(1)]
    );
}

fn call(n: u128, agent: u128, seconds: i64) -> AgentCall {
    AgentCall::new(
        cid(n),
        NewAgentCall {
            agent: aid(agent),
            operation: OperationId::new("admin.orders.list").unwrap(),
            mode: CallMode::Read,
            status: 200,
            error_code: None,
            duration_ms: 0,
            at: at(seconds),
        },
    )
    .unwrap()
}

/// Runs every `AgentCallRepo` scenario, each on fresh ports from `make`.
pub async fn agent_call_repo<S: HasAgentCalls>(make: impl AsyncFn() -> S) {
    scenario("agent calls: every field round-trips");
    call_round_trip(&make().await).await;
    scenario("agent calls: an ID is stored once");
    call_duplicate(&make().await).await;
    scenario("agent calls: an agent's calls page newest first, without another agent's");
    call_lists(&make().await).await;
    scenario("agent calls: prune deletes calls made before a time, up to a limit");
    call_prune(&make().await).await;
}

async fn calls_of<S: HasAgentCalls>(
    ports: &S,
    agent: u128,
    after: Option<u128>,
    limit: u32,
) -> (Vec<AgentCallId>, bool) {
    let found = ports
        .agent_calls()
        .list_for_agent(aid(agent), page(after.map(cid), limit))
        .await
        .unwrap();
    (
        found.items.iter().map(AgentCall::id).collect(),
        found.has_more,
    )
}

async fn call_round_trip<S: HasAgentCalls>(ports: &S) {
    let repo = ports.agent_calls();
    let full = AgentCall::new(
        cid(1),
        NewAgentCall {
            agent: aid(1),
            operation: OperationId::new("app.app_reviews.import-csv").unwrap(),
            mode: CallMode::Propose,
            status: 409,
            error_code: Some("insufficient_stock".to_string()),
            duration_ms: u32::MAX,
            at: at(1),
        },
    )
    .unwrap();
    repo.insert(&full).await.unwrap();
    repo.insert(&call(2, 1, 2)).await.unwrap();
    let found = repo
        .list_for_agent(aid(1), page(None, 10))
        .await
        .unwrap()
        .items;
    assert_eq!(found, vec![call(2, 1, 2), full]);
}

async fn call_duplicate<S: HasAgentCalls>(ports: &S) {
    let repo = ports.agent_calls();
    repo.insert(&call(1, 1, 1)).await.unwrap();
    assert_eq!(
        repo.insert(&call(1, 2, 2)).await,
        Err(RepoError::Duplicate(keys::AGENT_CALL_ID))
    );
}

async fn call_lists<S: HasAgentCalls>(ports: &S) {
    let repo = ports.agent_calls();
    for (n, agent) in [(1, 1), (2, 2), (3, 1), (4, 1)] {
        repo.insert(&call(n, agent, n as i64)).await.unwrap();
    }
    assert_eq!(
        calls_of(ports, 1, None, 2).await,
        (vec![cid(4), cid(3)], true)
    );
    assert_eq!(calls_of(ports, 1, Some(3), 2).await, (vec![cid(1)], false));
    assert_eq!(calls_of(ports, 2, None, 10).await, (vec![cid(2)], false));
    assert_eq!(calls_of(ports, 3, None, 10).await, (vec![], false));
}

async fn call_prune<S: HasAgentCalls>(ports: &S) {
    let repo = ports.agent_calls();
    for n in 1..=4 {
        repo.insert(&call(n, 1, n as i64)).await.unwrap();
    }
    assert_eq!(repo.prune_before(at(1), 10).await, Ok(0), "strictly before");
    assert_eq!(repo.prune_before(at(4), 2).await, Ok(2));
    assert_eq!(repo.prune_before(at(4), 10).await, Ok(1));
    assert_eq!(calls_of(ports, 1, None, 10).await, (vec![cid(4)], false));
}
