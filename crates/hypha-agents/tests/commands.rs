//! Agents, proposals and the call log over a store in memory.

use core::time::Duration;

use hypha_agents::{Agent, AgentSettings, Proposal};
use hypha_agents::{
    AgentError, AgentFilter, AgentKind, AgentRepo, CallMode, ClientInput, CommandError,
    ConnectAgent, CreateAgent, GrantError, GrantInput, ListProposals, NewAgentCall, OperationId,
    Outcome, ProposalError, ProposalRequest, ProposalStatus, Propose, UpdateAgent, WriteMode,
    WritesCap, active_agent, approve_proposal, create_autonomous_agent, create_connected_agent,
    create_proposal, effective, expire_proposals, get_agent, get_proposal, list_agent_calls,
    list_agents, list_proposals, prune_agent_calls, record_agent_use, record_applied, record_call,
    record_failed, reject_proposal, revoke_agent, update_agent,
};
use hypha_core::{
    Actor, AgentId, ApiKeyId, AuthError, Ctx, IdempotencyError, PageRequest, Progress, ProposalId,
    Scope, Timestamp, UserId, Versioned,
};
use hypha_testkit::{FixedClock, InMemoryPorts, SequentialIds};
use pollster::block_on;
use serde_json::json;
use uuid::Uuid;

fn user(n: u128) -> UserId {
    UserId::from_uuid(Uuid::from_u128(1000 + n))
}

fn staff() -> Ctx {
    Ctx::new(Actor::User(user(1)), Scope::ALL.to_vec())
}

fn key(scopes: Vec<Scope>) -> Ctx {
    Ctx::new(
        Actor::ApiKey(ApiKeyId::from_uuid(Uuid::from_u128(77))),
        scopes,
    )
}

fn grant(read: &[&str], write: &[(&str, &str)]) -> GrantInput {
    GrantInput {
        read: read.iter().map(|s| s.to_string()).collect(),
        write: write
            .iter()
            .map(|(s, m)| (s.to_string(), m.to_string()))
            .collect(),
        customer_data: Vec::new(),
        operations: None,
    }
}

struct World {
    ports: InMemoryPorts,
    ids: SequentialIds,
    clock: FixedClock,
    settings: AgentSettings,
}

impl World {
    fn new() -> Self {
        Self {
            ports: InMemoryPorts::default(),
            ids: SequentialIds::default(),
            clock: FixedClock::at(Timestamp::from_unix_micros(1_000_000)),
            settings: AgentSettings::default(),
        }
    }

    fn create(&self, ctx: &Ctx, g: GrantInput) -> Result<Agent, CommandError> {
        let input = CreateAgent {
            name: "Restocker".to_string(),
            grant: g,
        };
        block_on(create_autonomous_agent(ctx, &self.ports, &self.ids, input))
    }

    /// An autonomous agent proposing product writes and applying inventory ones.
    fn agent(&self) -> Agent {
        self.create(
            &staff(),
            grant(
                &["orders:read"],
                &[("products:write", "propose"), ("inventory:write", "apply")],
            ),
        )
        .unwrap()
    }

    /// The context the host would build for `agent`'s requests.
    fn as_agent(&self, agent: &Agent) -> Ctx {
        let permissions = effective(agent.grant(), WritesCap::Apply, None);
        Ctx::new(Actor::Agent(agent.id()), permissions.scopes())
    }

    fn propose(&self, ctx: &Ctx, key: Option<&str>) -> Result<Proposal, CommandError> {
        self.propose_body(ctx, key, json!({"title": "Mug"}))
    }

    fn propose_body(
        &self,
        ctx: &Ctx,
        key: Option<&str>,
        body: serde_json::Value,
    ) -> Result<Proposal, CommandError> {
        let mut request = ProposalRequest::default();
        request.params.insert(
            "id".to_string(),
            json!("prod_0190f1a2b3c47d8e9f00112233445566"),
        );
        request.body = Some(body);
        let input = Propose {
            operation: "admin.products.update".to_string(),
            request,
            idempotency_key: key.map(str::to_string),
            preview: json!({"complete": true, "events": []}),
            digest: "sha256:0f".to_string(),
            scopes: vec![Scope::ProductsWrite],
        };
        block_on(create_proposal(
            ctx,
            &self.ports,
            &self.ids,
            &self.clock,
            &self.settings,
            input,
        ))
        .map(|p| p.value)
    }

    fn types(&self) -> Vec<String> {
        self.ports
            .outbox
            .events()
            .into_iter()
            .map(|e| e.event_type)
            .collect()
    }

    fn later(&self, by: Duration) {
        self.clock.set(self.clock_now().plus(by));
    }

    fn clock_now(&self) -> Timestamp {
        hypha_core::Clock::now(&self.clock)
    }
}

fn ok() -> Outcome {
    Outcome {
        status: 200,
        body: json!({"id": "prod_1"}),
    }
}

#[test]
fn staff_create_an_autonomous_agent() {
    let w = World::new();
    let agent = w.agent();
    assert_eq!(agent.kind(), AgentKind::Autonomous);
    assert!(agent.is_active());
    assert_eq!(agent.version().value(), 1);
    assert_eq!(
        agent.grant().write(),
        [
            (Scope::ProductsWrite, WriteMode::Propose),
            (Scope::InventoryWrite, WriteMode::Apply)
        ]
    );
    assert_eq!(w.types(), ["agent.created"]);
    assert_eq!(
        block_on(get_agent(&staff(), &w.ports, agent.id())),
        Ok(agent)
    );
}

#[test]
fn creating_needs_agents_write() {
    let w = World::new();
    assert_eq!(
        w.create(&key(vec![Scope::AgentsRead]), grant(&[], &[]))
            .err(),
        Some(CommandError::Auth(AuthError::MissingScope(
            Scope::AgentsWrite
        )))
    );
}

#[test]
fn never_granted_scopes_are_refused_by_name() {
    let w = World::new();
    assert_eq!(
        w.create(&staff(), grant(&[], &[("users:write", "propose")]))
            .err(),
        Some(CommandError::Grant(GrantError::NotGrantable(
            Scope::UsersWrite
        )))
    );
    assert_eq!(
        w.create(&staff(), grant(&[], &[("customers:erase", "apply")]))
            .err(),
        Some(CommandError::Grant(GrantError::ProposeOnly(
            Scope::CustomersErase
        )))
    );
}

#[test]
fn nobody_grants_a_scope_they_lack() {
    let w = World::new();
    let ctx = key(vec![Scope::AgentsWrite, Scope::ProductsRead]);
    assert_eq!(
        w.create(&ctx, grant(&["orders:read"], &[])).err(),
        Some(CommandError::CannotGrant(Scope::OrdersRead))
    );
    // A write scope stands for its read scope, as for any caller.
    let writer = key(vec![Scope::AgentsWrite, Scope::OrdersWrite]);
    assert!(w.create(&writer, grant(&["orders:read"], &[])).is_ok());
}

#[test]
fn only_a_staff_session_grants_apply() {
    let w = World::new();
    let all = key(Scope::ALL.to_vec());
    assert_eq!(
        w.create(&all, grant(&[], &[("orders:write", "apply")]))
            .err(),
        Some(CommandError::CannotGrantApply(Scope::OrdersWrite))
    );
    assert!(
        w.create(&all, grant(&[], &[("orders:write", "propose")]))
            .is_ok()
    );
    assert!(
        w.create(&staff(), grant(&[], &[("orders:write", "apply")]))
            .is_ok()
    );
}

#[test]
fn a_connected_agent_acts_for_the_staff_member_who_connected_it() {
    let w = World::new();
    let input = || ConnectAgent {
        name: "Claude".to_string(),
        client: Some(ClientInput {
            client_id: "https://claude.ai/oauth/mcp-client.json".to_string(),
            name: "Claude".to_string(),
        }),
        grant: grant(&["products:read"], &[]),
    };
    let agent = block_on(create_connected_agent(&staff(), &w.ports, &w.ids, input())).unwrap();
    assert_eq!(agent.kind(), AgentKind::Connected { user: user(1) });
    assert_eq!(
        agent.client().map(|c| c.client_id()),
        Some("https://claude.ai/oauth/mcp-client.json")
    );
    assert_eq!(
        block_on(create_connected_agent(
            &key(Scope::ALL.to_vec()),
            &w.ports,
            &w.ids,
            input()
        ))
        .err(),
        Some(CommandError::SessionRequired)
    );
    // A staff member without `agents:write` connects their own client too, granting
    // only what their role holds.
    let clerk = Ctx::new(Actor::User(user(2)), vec![Scope::ProductsRead]);
    let own = block_on(create_connected_agent(&clerk, &w.ports, &w.ids, input())).unwrap();
    assert_eq!(own.kind(), AgentKind::Connected { user: user(2) });
    let wider = ConnectAgent {
        grant: grant(&["orders:read"], &[]),
        ..input()
    };
    assert_eq!(
        block_on(create_connected_agent(&clerk, &w.ports, &w.ids, wider)).err(),
        Some(CommandError::CannotGrant(Scope::OrdersRead))
    );
    let mine = AgentFilter {
        user: Some(user(1)),
        include_revoked: false,
    };
    w.agent();
    let listed = block_on(list_agents(
        &staff(),
        &w.ports,
        &mine,
        PageRequest::default(),
    ))
    .unwrap();
    assert_eq!(listed.items, [agent]);
}

#[test]
fn one_update_is_one_event_listing_what_changed() {
    let w = World::new();
    let agent = w.agent();
    let input = UpdateAgent {
        name: Some("Night restocker".to_string()),
        grant: Some(grant(&["orders:read"], &[("inventory:write", "propose")])),
        enabled: Some(false),
        operations: None,
    };
    let updated = block_on(update_agent(&staff(), &w.ports, agent.id(), input)).unwrap();
    assert_eq!(updated.name(), "Night restocker");
    assert!(!updated.enabled());
    assert_eq!(updated.version().value(), 2);
    let events = w.ports.outbox.events();
    assert_eq!(events.len(), 2);
    assert_eq!(
        events[1].data,
        json!({"agent": agent.id().to_string(), "changed": ["name", "grant", "enabled"]})
    );
}

#[test]
fn an_update_that_changes_nothing_writes_nothing() {
    let w = World::new();
    let agent = w.agent();
    let input = UpdateAgent {
        name: Some(" Restocker ".to_string()),
        enabled: Some(true),
        ..UpdateAgent::default()
    };
    let same = block_on(update_agent(&staff(), &w.ports, agent.id(), input)).unwrap();
    assert_eq!(same.version().value(), 1);
    assert_eq!(w.types(), ["agent.created"]);
}

#[test]
fn narrowing_the_allowlist_needs_no_scopes_but_changing_scopes_does() {
    let w = World::new();
    let agent = w.agent();
    let manager = key(vec![Scope::AgentsWrite]);
    let ops = UpdateAgent {
        operations: Some(Some(vec!["admin.products.update".to_string()])),
        ..UpdateAgent::default()
    };
    let narrowed = block_on(update_agent(&manager, &w.ports, agent.id(), ops)).unwrap();
    assert_eq!(
        narrowed.grant().operations(),
        Some(&[OperationId::new("admin.products.update").unwrap()][..])
    );
    let wider = UpdateAgent {
        grant: Some(grant(&["orders:read"], &[])),
        ..UpdateAgent::default()
    };
    assert_eq!(
        block_on(update_agent(&manager, &w.ports, agent.id(), wider)).err(),
        Some(CommandError::CannotGrant(Scope::OrdersRead))
    );
    let bad = UpdateAgent {
        operations: Some(Some(vec!["products".to_string()])),
        ..UpdateAgent::default()
    };
    assert_eq!(
        block_on(update_agent(&staff(), &w.ports, agent.id(), bad)).err(),
        Some(CommandError::Operation(hypha_agents::OperationIdError(
            "products".to_string()
        )))
    );
}

#[test]
fn a_revoked_agent_stops_and_changes_no_more() {
    let w = World::new();
    let agent = w.agent();
    let revoked = block_on(revoke_agent(&staff(), &w.ports, &w.clock, agent.id())).unwrap();
    assert_eq!(revoked.revoked_at(), Some(w.clock_now()));
    assert_eq!(w.types(), ["agent.created", "agent.revoked"]);
    assert_eq!(
        block_on(active_agent(&w.ports, agent.id())).err(),
        Some(CommandError::Agent(AgentError::Revoked(agent.id())))
    );
    let rename = UpdateAgent {
        name: Some("x".to_string()),
        ..UpdateAgent::default()
    };
    assert_eq!(
        block_on(update_agent(&staff(), &w.ports, agent.id(), rename)).err(),
        Some(CommandError::Agent(AgentError::Revoked(agent.id())))
    );
    let listed = |include_revoked| {
        let filter = AgentFilter {
            user: None,
            include_revoked,
        };
        block_on(list_agents(
            &staff(),
            &w.ports,
            &filter,
            PageRequest::default(),
        ))
        .unwrap()
        .items
        .len()
    };
    assert_eq!((listed(false), listed(true)), (0, 1));
}

#[test]
fn a_disabled_agent_is_refused_until_enabled() {
    let w = World::new();
    let agent = w.agent();
    let off = UpdateAgent {
        enabled: Some(false),
        ..UpdateAgent::default()
    };
    block_on(update_agent(&staff(), &w.ports, agent.id(), off)).unwrap();
    assert_eq!(
        block_on(active_agent(&w.ports, agent.id())).err(),
        Some(CommandError::AgentDisabled(agent.id()))
    );
    let missing = AgentId::from_uuid(Uuid::from_u128(999));
    assert_eq!(
        block_on(active_agent(&w.ports, missing)).err(),
        Some(CommandError::AgentNotFound(missing))
    );
}

#[test]
fn reading_agents_needs_agents_read() {
    let w = World::new();
    let agent = w.agent();
    assert_eq!(
        block_on(get_agent(&key(vec![]), &w.ports, agent.id())).err(),
        Some(CommandError::Auth(AuthError::MissingScope(
            Scope::AgentsRead
        )))
    );
}

#[test]
fn use_is_recorded_at_most_once_a_minute() {
    let w = World::new();
    let agent = w.agent();
    let now = w.clock_now();
    block_on(record_agent_use(&w.ports, &agent, now)).unwrap();
    let used = block_on(w.ports.agents.get(agent.id())).unwrap().unwrap();
    assert_eq!(used.last_used_at(), Some(now));
    assert_eq!(used.version().value(), 1, "use bumps no version");
    let soon = now.plus(Duration::from_secs(59));
    block_on(record_agent_use(&w.ports, &used, soon)).unwrap();
    let still = block_on(w.ports.agents.get(agent.id())).unwrap().unwrap();
    assert_eq!(still.last_used_at(), Some(now));
    let later = now.plus(Duration::from_secs(60));
    block_on(record_agent_use(&w.ports, &still, later)).unwrap();
    let moved = block_on(w.ports.agents.get(agent.id())).unwrap().unwrap();
    assert_eq!(moved.last_used_at(), Some(later));
}

#[test]
fn an_agent_proposes_a_write_it_may_propose() {
    let w = World::new();
    let agent = w.agent();
    let proposal = w.propose(&w.as_agent(&agent), None).unwrap();
    assert_eq!(proposal.agent(), agent.id());
    assert_eq!(proposal.status(), ProposalStatus::Pending);
    assert_eq!(
        proposal.expires_at(),
        w.clock_now().plus(w.settings.proposal_lifetime)
    );
    assert_eq!(w.types(), ["agent.created", "proposal.created"]);
}

#[test]
fn only_an_agent_proposes_and_only_with_the_scopes() {
    let w = World::new();
    let agent = w.agent();
    assert_eq!(
        w.propose(&staff(), None).err(),
        Some(CommandError::AgentRequired)
    );
    let reader = Ctx::new(Actor::Agent(agent.id()), vec![Scope::OrdersRead]);
    assert_eq!(
        w.propose(&reader, None).err(),
        Some(CommandError::Auth(AuthError::MissingScope(
            Scope::ProductsWrite
        )))
    );
    let off = UpdateAgent {
        enabled: Some(false),
        ..UpdateAgent::default()
    };
    block_on(update_agent(&staff(), &w.ports, agent.id(), off)).unwrap();
    assert_eq!(
        w.propose(&w.as_agent(&agent), None).err(),
        Some(CommandError::AgentDisabled(agent.id()))
    );
}

#[test]
fn a_retried_proposal_answers_the_first_and_a_reused_key_is_refused() {
    let w = World::new();
    let agent = w.agent();
    let ctx = w.as_agent(&agent);
    let first = w.propose(&ctx, Some("retry-1")).unwrap();
    assert_eq!(w.propose(&ctx, Some("retry-1")), Ok(first.clone()));
    assert_eq!(
        w.propose_body(&ctx, Some("retry-1"), json!({"title": "Cup"}))
            .err(),
        Some(CommandError::Idempotency(IdempotencyError::KeyReused))
    );
    assert_eq!(
        w.propose(&ctx, Some("bad key")).err(),
        Some(CommandError::Idempotency(IdempotencyError::InvalidKey))
    );
    let page = block_on(list_proposals(
        &staff(),
        &w.ports,
        ListProposals::default(),
        PageRequest::default(),
    ))
    .unwrap();
    assert_eq!(page.items, [first]);
}

#[test]
fn a_racing_retry_that_loses_on_the_key_answers_the_winner() {
    let w = World::new();
    let agent = w.agent();
    let ctx = w.as_agent(&agent);
    let first = w.propose(&ctx, Some("race")).unwrap();
    // The lookup before the insert misses the first one, as if it committed just after.
    w.ports.agents.hide_next_proposal_lookups(1);
    assert_eq!(w.propose(&ctx, Some("race")), Ok(first));
}

#[test]
fn a_staff_member_holding_its_scopes_approves_then_records_the_result() {
    let w = World::new();
    let agent = w.agent();
    let proposal = w.propose(&w.as_agent(&agent), None).unwrap();
    let approved = block_on(approve_proposal(
        &staff(),
        &w.ports,
        &w.clock,
        proposal.id(),
    ));
    assert_eq!(approved, Ok(proposal.clone()));
    let applied = block_on(record_applied(
        &staff(),
        &w.ports,
        &w.clock,
        proposal.id(),
        ok(),
    ))
    .unwrap();
    assert_eq!(applied.status(), ProposalStatus::Applied);
    assert_eq!(applied.decided_by(), Some(user(1)));
    assert_eq!(applied.response().map(|r| r.status()), Some(200));
    assert_eq!(
        w.types(),
        ["agent.created", "proposal.created", "proposal.decided"]
    );
    assert_eq!(
        block_on(reject_proposal(
            &staff(),
            &w.ports,
            &w.clock,
            proposal.id(),
            None
        ))
        .err(),
        Some(CommandError::Proposal(ProposalError::Decided(
            ProposalStatus::Applied
        )))
    );
}

#[test]
fn deciding_takes_a_session_with_every_scope() {
    let w = World::new();
    let agent = w.agent();
    let proposal = w.propose(&w.as_agent(&agent), None).unwrap();
    let id = proposal.id();
    assert_eq!(
        block_on(approve_proposal(
            &key(Scope::ALL.to_vec()),
            &w.ports,
            &w.clock,
            id
        ))
        .err(),
        Some(CommandError::SessionRequired)
    );
    let reader = Ctx::new(Actor::User(user(2)), vec![Scope::ProductsRead]);
    assert_eq!(
        block_on(approve_proposal(&reader, &w.ports, &w.clock, id)).err(),
        Some(CommandError::Auth(AuthError::MissingScope(
            Scope::ProductsWrite
        )))
    );
    assert_eq!(
        block_on(reject_proposal(&reader, &w.ports, &w.clock, id, None)).err(),
        Some(CommandError::Auth(AuthError::MissingScope(
            Scope::ProductsWrite
        )))
    );
}

#[test]
fn an_expired_proposal_is_not_approved() {
    let w = World::new();
    let agent = w.agent();
    let proposal = w.propose(&w.as_agent(&agent), None).unwrap();
    w.later(w.settings.proposal_lifetime);
    assert_eq!(
        block_on(approve_proposal(
            &staff(),
            &w.ports,
            &w.clock,
            proposal.id()
        ))
        .err(),
        Some(CommandError::Proposal(ProposalError::Expired))
    );
}

#[test]
fn a_revoked_agents_proposals_are_not_approved() {
    let w = World::new();
    let agent = w.agent();
    let proposal = w.propose(&w.as_agent(&agent), None).unwrap();
    block_on(revoke_agent(&staff(), &w.ports, &w.clock, agent.id())).unwrap();
    assert_eq!(
        block_on(approve_proposal(
            &staff(),
            &w.ports,
            &w.clock,
            proposal.id()
        ))
        .err(),
        Some(CommandError::Agent(AgentError::Revoked(agent.id())))
    );
}

#[test]
fn a_failed_apply_and_a_rejection_are_recorded() {
    let w = World::new();
    let agent = w.agent();
    let ctx = w.as_agent(&agent);
    let one = w.propose(&ctx, None).unwrap();
    let stale = Outcome {
        status: 409,
        body: json!({"code": "preview_stale"}),
    };
    let failed = block_on(record_failed(&staff(), &w.ports, &w.clock, one.id(), stale)).unwrap();
    assert_eq!(failed.status(), ProposalStatus::Failed);
    let two = w.propose(&ctx, None).unwrap();
    let note = Some(" Not before the sale ".to_string());
    let rejected = block_on(reject_proposal(
        &staff(),
        &w.ports,
        &w.clock,
        two.id(),
        note,
    ))
    .unwrap();
    assert_eq!(rejected.note(), Some("Not before the sale"));
    let bad = Outcome {
        status: 42,
        body: json!(null),
    };
    let three = w.propose(&ctx, None).unwrap();
    assert_eq!(
        block_on(record_applied(
            &staff(),
            &w.ports,
            &w.clock,
            three.id(),
            bad
        ))
        .err(),
        Some(CommandError::Proposal(ProposalError::InvalidStatus(42)))
    );
}

#[test]
fn expiring_works_through_what_is_due_in_batches() {
    let w = World::new();
    let agent = w.agent();
    let ctx = w.as_agent(&agent);
    let first = w.propose(&ctx, None).unwrap();
    let second = w.propose(&ctx, None).unwrap();
    let decided = w.propose(&ctx, None).unwrap();
    block_on(reject_proposal(
        &staff(),
        &w.ports,
        &w.clock,
        decided.id(),
        None,
    ))
    .unwrap();
    w.later(Duration::from_secs(60));
    let young = w.propose(&ctx, None).unwrap();
    assert_eq!(
        block_on(expire_proposals(&w.ports, &w.clock, 5)),
        Ok(Progress {
            done: 0,
            more: false
        })
    );
    w.later(w.settings.proposal_lifetime - Duration::from_secs(60));
    assert_eq!(
        block_on(expire_proposals(&w.ports, &w.clock, 1)),
        Ok(Progress {
            done: 1,
            more: true
        })
    );
    assert_eq!(
        block_on(expire_proposals(&w.ports, &w.clock, 5)),
        Ok(Progress {
            done: 1,
            more: false
        })
    );
    let status = |id: ProposalId| {
        block_on(get_proposal(&staff(), &w.ports, id))
            .unwrap()
            .status()
    };
    assert_eq!(status(first.id()), ProposalStatus::Expired);
    assert_eq!(status(second.id()), ProposalStatus::Expired);
    assert_eq!(status(decided.id()), ProposalStatus::Rejected);
    assert_eq!(status(young.id()), ProposalStatus::Pending);
}

#[test]
fn an_agent_reads_only_its_own_proposals() {
    let w = World::new();
    let mine = w.agent();
    let other = w.agent();
    let p = w.propose(&w.as_agent(&mine), None).unwrap();
    let theirs = w.propose(&w.as_agent(&other), None).unwrap();
    assert_eq!(
        block_on(get_proposal(&w.as_agent(&mine), &w.ports, p.id())),
        Ok(p.clone())
    );
    assert_eq!(
        block_on(get_proposal(&w.as_agent(&mine), &w.ports, theirs.id())).err(),
        Some(CommandError::ProposalNotFound(theirs.id()))
    );
    let asked_for_theirs = ListProposals {
        agent: Some(other.id()),
        ..ListProposals::default()
    };
    let page = block_on(list_proposals(
        &w.as_agent(&mine),
        &w.ports,
        asked_for_theirs,
        PageRequest::default(),
    ))
    .unwrap();
    assert_eq!(page.items, [p]);
    assert_eq!(
        block_on(get_proposal(&key(vec![]), &w.ports, theirs.id())).err(),
        Some(CommandError::Auth(AuthError::MissingScope(
            Scope::AgentsRead
        )))
    );
}

#[test]
fn approvable_lists_what_the_caller_could_approve() {
    let w = World::new();
    let agent = w.agent();
    w.propose(&w.as_agent(&agent), None).unwrap();
    let list = |ctx: &Ctx| {
        let input = ListProposals {
            approvable: true,
            status: Some(ProposalStatus::Pending),
            ..ListProposals::default()
        };
        block_on(list_proposals(ctx, &w.ports, input, PageRequest::default()))
            .unwrap()
            .items
            .len()
    };
    let reader = Ctx::new(Actor::User(user(2)), vec![Scope::AgentsRead]);
    let editor = Ctx::new(
        Actor::User(user(3)),
        vec![Scope::AgentsRead, Scope::ProductsWrite],
    );
    assert_eq!((list(&reader), list(&editor)), (0, 1));
}

fn call(agent: AgentId, at: Timestamp, status: u16) -> NewAgentCall {
    NewAgentCall {
        agent,
        operation: OperationId::new("admin.orders.list").unwrap(),
        mode: CallMode::Read,
        status,
        error_code: None,
        duration_ms: 12,
        at,
    }
}

#[test]
fn calls_are_logged_listed_newest_first_and_pruned_by_age() {
    let w = World::new();
    let agent = w.agent();
    let now = w.clock_now();
    let old = block_on(record_call(&w.ports, &w.ids, call(agent.id(), now, 200))).unwrap();
    let new = block_on(record_call(
        &w.ports,
        &w.ids,
        call(agent.id(), now.plus(w.settings.keep_calls), 200),
    ))
    .unwrap();
    let page = block_on(list_agent_calls(
        &staff(),
        &w.ports,
        agent.id(),
        PageRequest::default(),
    ))
    .unwrap();
    assert_eq!(page.items, [new.clone(), old]);
    w.later(w.settings.keep_calls + Duration::from_secs(1));
    assert_eq!(
        block_on(prune_agent_calls(&w.ports, &w.clock, &w.settings, 10)),
        Ok(Progress {
            done: 1,
            more: false
        })
    );
    assert_eq!(w.ports.agents.calls(), [new]);
    assert_eq!(
        block_on(record_call(&w.ports, &w.ids, call(agent.id(), now, 99))).err(),
        Some(CommandError::Call(hypha_agents::CallError::InvalidStatus(
            99
        )))
    );
    let missing = AgentId::from_uuid(Uuid::from_u128(999));
    assert_eq!(
        block_on(list_agent_calls(
            &staff(),
            &w.ports,
            missing,
            PageRequest::default()
        ))
        .err(),
        Some(CommandError::AgentNotFound(missing))
    );
}
