use std::{
    collections::HashMap,
    sync::{Mutex, MutexGuard},
};

use hypha_agents::{
    Agent, AgentCall, AgentCallRepo, AgentFilter, AgentRepo, Proposal, ProposalFilter,
    ProposalRepo, ProposalStatus, keys,
};
use hypha_core::{
    AgentCallId, AgentId, IdempotencyKey, Page, PageRequest, ProposalId, Recorded, RepoError,
    Timestamp, Versioned, stamp_insert, stamp_update,
};

use crate::{
    InMemoryOutbox,
    table::{Table, missing, no_unique_keys},
};

/// Agents, their proposals and their call log, in memory: one value since a proposal
/// needs its agent (a foreign key in a database).
#[derive(Default)]
pub struct InMemoryAgents {
    pub(crate) agents: Table<AgentId, Agent>,
    pub(crate) proposals: Table<ProposalId, Proposal>,
    calls: Mutex<Vec<AgentCall>>,
    outbox: InMemoryOutbox,
}

impl InMemoryAgents {
    /// Agents writing their events to `outbox` (`InMemoryPorts` shares one).
    pub fn sharing(outbox: &InMemoryOutbox) -> Self {
        Self {
            outbox: outbox.clone(),
            ..Self::default()
        }
    }

    /// Every call recorded, oldest first: for tests that check what was logged.
    pub fn calls(&self) -> Vec<AgentCall> {
        let mut calls = self.lock_calls().clone();
        calls.sort_by_key(AgentCall::id);
        calls
    }

    /// Makes the next `n` proposal lookups by something other than the ID see none, to
    /// lose the race between a command's check of a key and its insert.
    pub fn hide_next_proposal_lookups(&self, n: usize) {
        self.proposals.hide_next_lookups(n);
    }

    fn lock_calls(&self) -> MutexGuard<'_, Vec<AgentCall>> {
        self.calls.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// `agent` as written by an update: everything but `last_used_at`, which stays as stored
/// (only `record_use` writes it).
fn keeping_use(agent: &Agent, stored: Option<&Agent>) -> Agent {
    let mut s = agent.snapshot();
    s.last_used_at = stored.and_then(Agent::last_used_at);
    // The snapshot came from a valid agent: restoring it can't fail.
    Agent::restore(s).unwrap_or_else(|e| unreachable!("a valid agent: {e}"))
}

/// `AgentRepo::update` on locked rows, for the repo and batches.
pub(crate) fn update_agent(
    rows: &mut HashMap<AgentId, Agent>,
    agent: &mut Agent,
) -> Result<(), RepoError> {
    let mut row = keeping_use(agent, rows.get(&agent.id()));
    crate::table::update_row(rows, agent.id(), &mut row, &no_unique_keys)?;
    agent.set_version(row.version());
    Ok(())
}

/// One proposal per agent and idempotency key.
pub(crate) fn unique_proposal(new: &Proposal, other: &Proposal) -> Option<&'static str> {
    let same_key = new.idempotency_key().is_some()
        && new.agent() == other.agent()
        && new.idempotency_key() == other.idempotency_key();
    same_key.then_some(keys::PROPOSAL_KEY)
}

/// `ProposalRepo::insert` on locked rows: its agent must exist.
pub(crate) fn insert_proposal(
    agents: &HashMap<AgentId, Agent>,
    proposals: &mut HashMap<ProposalId, Proposal>,
    proposal: &mut Proposal,
) -> Result<(), RepoError> {
    if !agents.contains_key(&proposal.agent()) {
        return Err(missing("agent", proposal.agent()));
    }
    crate::table::insert_row(
        proposals,
        proposal.id(),
        proposal,
        keys::PROPOSAL_ID,
        &unique_proposal,
    )
}

impl AgentRepo for InMemoryAgents {
    async fn get(&self, id: AgentId) -> Result<Option<Agent>, RepoError> {
        Ok(self.agents.get(&id))
    }

    async fn list(
        &self,
        filter: &AgentFilter,
        page: PageRequest<AgentId>,
    ) -> Result<Page<Agent>, RepoError> {
        Ok(self.agents.page(
            |a| {
                filter.user.is_none_or(|u| a.kind().user() == Some(u))
                    && (filter.include_revoked || a.revoked_at().is_none())
            },
            page,
            Agent::clone,
        ))
    }

    async fn insert(&self, agent: &mut Agent) -> Result<(), RepoError> {
        stamp_insert(agent, self.outbox.now());
        let events = agent.take_events();
        self.agents
            .insert(agent.id(), agent, keys::AGENT_ID, &no_unique_keys)?;
        self.outbox.write(events, Some(agent.version()));
        Ok(())
    }

    async fn update(&self, agent: &mut Agent) -> Result<(), RepoError> {
        stamp_update(agent, self.outbox.now());
        let events = agent.take_events();
        update_agent(&mut self.agents.lock(), agent)?;
        self.outbox.write(events, Some(agent.version()));
        Ok(())
    }

    async fn record_use(&self, id: AgentId, at: Timestamp) -> Result<(), RepoError> {
        let mut rows = self.agents.lock();
        let Some(stored) = rows.get(&id) else {
            return Ok(());
        };
        if stored.last_used_at().is_some_and(|last| last >= at) {
            return Ok(());
        }
        let mut s = stored.snapshot();
        s.last_used_at = Some(at);
        let used = Agent::restore(s).map_err(|e| RepoError::Backend(e.to_string()))?;
        rows.insert(id, used);
        Ok(())
    }
}

impl ProposalRepo for InMemoryAgents {
    async fn get(&self, id: ProposalId) -> Result<Option<Proposal>, RepoError> {
        Ok(self.proposals.get(&id))
    }

    async fn find_by_key(
        &self,
        agent: AgentId,
        key: &IdempotencyKey,
    ) -> Result<Option<Proposal>, RepoError> {
        Ok(self
            .proposals
            .find(|p| p.agent() == agent && p.idempotency_key() == Some(key)))
    }

    async fn list(
        &self,
        filter: &ProposalFilter,
        page: PageRequest<ProposalId>,
    ) -> Result<Page<Proposal>, RepoError> {
        Ok(self.proposals.page(
            |p| {
                filter.agent.is_none_or(|a| p.agent() == a)
                    && filter.status.is_none_or(|s| p.status() == s)
                    && filter
                        .within
                        .as_ref()
                        .is_none_or(|within| p.scopes().iter().all(|s| within.contains(s)))
            },
            page,
            Proposal::clone,
        ))
    }

    async fn due(&self, now: Timestamp, limit: u32) -> Result<Vec<Proposal>, RepoError> {
        let mut due: Vec<Proposal> = self
            .proposals
            .all()
            .into_iter()
            .filter(|p| p.status() == ProposalStatus::Pending && p.expires_at() <= now)
            .collect();
        due.sort_by_key(|p| (p.expires_at(), p.id()));
        due.truncate(limit as usize);
        Ok(due)
    }

    async fn insert(&self, proposal: &mut Proposal) -> Result<(), RepoError> {
        stamp_insert(proposal, self.outbox.now());
        let events = proposal.take_events();
        let agents = self.agents.lock();
        insert_proposal(&agents, &mut self.proposals.lock(), proposal)?;
        self.outbox.write(events, Some(proposal.version()));
        Ok(())
    }

    async fn update(&self, proposal: &mut Proposal) -> Result<(), RepoError> {
        stamp_update(proposal, self.outbox.now());
        let events = proposal.take_events();
        self.proposals
            .update(proposal.id(), proposal, &unique_proposal)?;
        self.outbox.write(events, Some(proposal.version()));
        Ok(())
    }
}

impl AgentCallRepo for InMemoryAgents {
    async fn insert(&self, call: &AgentCall) -> Result<(), RepoError> {
        let mut calls = self.lock_calls();
        if calls.iter().any(|c| c.id() == call.id()) {
            return Err(RepoError::Duplicate(keys::AGENT_CALL_ID));
        }
        calls.push(call.clone());
        Ok(())
    }

    async fn list_for_agent(
        &self,
        agent: AgentId,
        page: PageRequest<AgentCallId>,
    ) -> Result<Page<AgentCall>, RepoError> {
        let mut found: Vec<AgentCall> = self
            .lock_calls()
            .iter()
            .filter(|c| c.agent() == agent && page.after.is_none_or(|after| c.id() < after))
            .cloned()
            .collect();
        found.sort_by_key(|c| std::cmp::Reverse(c.id()));
        found.truncate(page.limit.get() as usize + 1);
        Ok(Page::from_overfetch(found, page.limit))
    }

    async fn prune_before(&self, before: Timestamp, limit: u32) -> Result<u32, RepoError> {
        let mut calls = self.lock_calls();
        let mut old: Vec<AgentCallId> = calls
            .iter()
            .filter(|c| c.created_at() < before)
            .map(AgentCall::id)
            .collect();
        old.truncate(limit as usize);
        calls.retain(|c| !old.contains(&c.id()));
        Ok(old.len() as u32)
    }
}
