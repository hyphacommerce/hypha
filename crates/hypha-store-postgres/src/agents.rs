//! Admin agents, their proposals and their call log (`0067_agents.sql`).

use hypha_agents::{
    Agent, AgentCall, AgentCallRepo, AgentClient, AgentFilter, AgentRepo, AgentSnapshot, CallMode,
    Grant, NewAgentCall, OperationId, Proposal, ProposalFilter, ProposalRepo, ProposalRequest,
    ProposalResponse, ProposalSnapshot, ProposalStatus, WriteMode,
};
use hypha_core::{
    AgentCallId, AgentId, IdempotencyKey, Page, PageLimit, PageRequest, ProposalId, Recorded,
    RepoError, Timestamp, UserId, Version, Versioned, stamp_insert, stamp_update,
};
use tokio_postgres::{Row, types::Type};
use uuid::Uuid;

use crate::{
    columns::{
        Param, col, micros, opt_micros, opt_ts, scopes_from_db, scopes_to_db, stamp_columns,
        stamps_from_row, ts, version_from_db, version_to_db, write_stamps,
    },
    error::{corrupt, repo_error},
    outbox::insert_events,
    store::{PgConnection, PgStore, Queries, in_transaction},
};

fn agent_columns() -> String {
    format!(
        "id, version, name, user_id, client_id, client_name, read_scopes, propose_scopes, \
         apply_scopes, customer_data, operations, enabled, {}, {}, {}",
        micros("last_used_at"),
        micros("revoked_at"),
        stamp_columns("")
    )
}

fn agent_from_row(row: &Row) -> Result<Agent, RepoError> {
    let id = AgentId::from_uuid(col(row, "id")?);
    let bad = |e: &dyn std::fmt::Display| corrupt(format!("agent {id}: {e}"));
    let client = match (
        col::<Option<String>>(row, "client_id")?,
        col::<Option<String>>(row, "client_name")?,
    ) {
        (Some(client_id), Some(name)) => {
            Some(AgentClient::new(&client_id, &name).map_err(|e| bad(&e))?)
        }
        _ => None,
    };
    let operations = col::<Option<Vec<String>>>(row, "operations")?
        .map(|ops| {
            ops.iter()
                .map(|o| OperationId::new(o))
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()
        .map_err(|e| bad(&e))?;
    let write = scopes_from_db(col(row, "propose_scopes")?)?
        .into_iter()
        .map(|s| (s, WriteMode::Propose))
        .chain(
            scopes_from_db(col(row, "apply_scopes")?)?
                .into_iter()
                .map(|s| (s, WriteMode::Apply)),
        )
        .collect();
    let grant = Grant::from_parts(
        scopes_from_db(col(row, "read_scopes")?)?,
        write,
        scopes_from_db(col(row, "customer_data")?)?,
        operations,
    )
    .map_err(|e| bad(&e))?;
    Agent::restore(AgentSnapshot {
        id,
        version: version_from_db(col(row, "version")?)?,
        stamps: stamps_from_row(row)?,
        name: col(row, "name")?,
        user: col::<Option<Uuid>>(row, "user_id")?.map(UserId::from_uuid),
        client,
        grant,
        enabled: col(row, "enabled")?,
        last_used_at: opt_ts(col(row, "last_used_at")?),
        revoked_at: opt_ts(col(row, "revoked_at")?),
    })
    .map_err(|e| bad(&e))
}

/// The grant's columns, in the order the INSERT and UPDATE below bind them.
struct GrantColumns {
    read: Vec<&'static str>,
    propose: Vec<&'static str>,
    apply: Vec<&'static str>,
    customer_data: Vec<&'static str>,
    operations: Option<Vec<String>>,
}

fn grant_columns(grant: &Grant) -> GrantColumns {
    GrantColumns {
        read: scopes_to_db(grant.read()),
        propose: scopes_to_db(&grant.writes_in(WriteMode::Propose)),
        apply: scopes_to_db(&grant.writes_in(WriteMode::Apply)),
        customer_data: scopes_to_db(grant.customer_data()),
        operations: grant
            .operations()
            .map(|ops| ops.iter().map(|o| o.to_string()).collect()),
    }
}

/// `AgentRepo::insert` inside the caller's transaction.
pub(crate) async fn insert_agent(tx: &impl Queries, s: &AgentSnapshot) -> Result<(), RepoError> {
    let g = grant_columns(&s.grant);
    let user = s.user.map(|u| u.as_uuid());
    let client_id = s.client.as_ref().map(|c| c.client_id().to_string());
    let client_name = s.client.as_ref().map(|c| c.name().to_string());
    tx.query_typed(
        &format!(
            "INSERT INTO agents (id, version, name, user_id, client_id, client_name,
                 read_scopes, propose_scopes, apply_scopes, customer_data, operations, enabled,
                 last_used_at, revoked_at)
             VALUES ($1, 1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, {}, {})",
            ts(12),
            ts(13)
        ),
        &[
            (&s.id.as_uuid(), Type::UUID),
            (&s.name, Type::TEXT),
            (&user, Type::UUID),
            (&client_id, Type::TEXT),
            (&client_name, Type::TEXT),
            (&g.read, Type::TEXT_ARRAY),
            (&g.propose, Type::TEXT_ARRAY),
            (&g.apply, Type::TEXT_ARRAY),
            (&g.customer_data, Type::TEXT_ARRAY),
            (&g.operations, Type::TEXT_ARRAY),
            (&s.enabled, Type::BOOL),
            (&opt_micros(s.last_used_at), Type::INT8),
            (&opt_micros(s.revoked_at), Type::INT8),
        ],
    )
    .await
    .map_err(repo_error)?;
    write_stamps(tx, "agents", "id", &s.id.as_uuid(), Type::UUID, s.stamps).await
}

/// `AgentRepo::update` inside the caller's transaction; the new version. Leaves
/// `last_used_at` alone (`record_use` writes it), and the kind and client, which never
/// change.
pub(crate) async fn update_agent(
    tx: &impl Queries,
    s: &AgentSnapshot,
) -> Result<Version, RepoError> {
    let g = grant_columns(&s.grant);
    let row = tx
        .query_typed_opt(
            &format!(
                "UPDATE agents SET version = version + 1, name = $3, read_scopes = $4,
                     propose_scopes = $5, apply_scopes = $6, customer_data = $7,
                     operations = $8, enabled = $9, revoked_at = {}
                 WHERE id = $1 AND version = $2 RETURNING version",
                ts(10)
            ),
            &[
                (&s.id.as_uuid(), Type::UUID),
                (&version_to_db(s.version)?, Type::INT8),
                (&s.name, Type::TEXT),
                (&g.read, Type::TEXT_ARRAY),
                (&g.propose, Type::TEXT_ARRAY),
                (&g.apply, Type::TEXT_ARRAY),
                (&g.customer_data, Type::TEXT_ARRAY),
                (&g.operations, Type::TEXT_ARRAY),
                (&s.enabled, Type::BOOL),
                (&opt_micros(s.revoked_at), Type::INT8),
            ],
        )
        .await
        .map_err(repo_error)?
        .ok_or(RepoError::StaleVersion)?;
    let version = version_from_db(col(&row, "version")?)?;
    write_stamps(tx, "agents", "id", &s.id.as_uuid(), Type::UUID, s.stamps).await?;
    Ok(version)
}

/// `WHERE …` for a newest-first page after `after`, given the conditions so far and their
/// parameters; appends the cursor and the limit (one more than the page, to see past it).
fn paged<'a>(
    mut conditions: Vec<String>,
    params: &mut Vec<Param<'a>>,
    after: &'a Option<Uuid>,
    fetch: &'a i64,
) -> String {
    if let Some(after) = after {
        params.push((after, Type::UUID));
        conditions.push(format!("id < ${}", params.len()));
    }
    params.push((fetch, Type::INT8));
    let filter = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };
    format!("{filter} ORDER BY id DESC LIMIT ${}", params.len())
}

fn overfetch(limit: PageLimit) -> i64 {
    i64::from(limit.get()) + 1
}

impl<C: PgConnection> AgentRepo for PgStore<C> {
    async fn get(&self, id: AgentId) -> Result<Option<Agent>, RepoError> {
        let mut conn = self.conn().await;
        let row = conn
            .db()
            .query_typed_opt(
                &format!("SELECT {} FROM agents WHERE id = $1", agent_columns()),
                &[(&id.as_uuid(), Type::UUID)],
            )
            .await
            .map_err(repo_error)?;
        row.as_ref().map(agent_from_row).transpose()
    }

    async fn list(
        &self,
        filter: &AgentFilter,
        page: PageRequest<AgentId>,
    ) -> Result<Page<Agent>, RepoError> {
        let user = filter.user.map(|u| u.as_uuid());
        let after = page.after.map(|id| id.as_uuid());
        let fetch = overfetch(page.limit);
        let mut params: Vec<Param<'_>> = Vec::new();
        let mut conditions = Vec::new();
        if let Some(user) = &user {
            params.push((user, Type::UUID));
            conditions.push(format!("user_id = ${}", params.len()));
        }
        if !filter.include_revoked {
            conditions.push("revoked_at IS NULL".to_string());
        }
        let rest = paged(conditions, &mut params, &after, &fetch);
        let mut conn = self.conn().await;
        // The primary key serves this; `agents_by_user` a person's.
        let found = conn
            .db()
            .query_typed(
                &format!("SELECT {} FROM agents {rest}", agent_columns()),
                &params,
            )
            .await
            .map_err(repo_error)?
            .iter()
            .map(agent_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Page::from_overfetch(found, page.limit))
    }

    async fn insert(&self, agent: &mut Agent) -> Result<(), RepoError> {
        stamp_insert(agent, self.now());
        let s = agent.snapshot();
        let events = agent.take_events();
        let stamp = self.event_stamp();
        in_transaction!(self, |tx| {
            insert_agent(tx, &s).await?;
            insert_events(tx, &events, Some(Version::NEW.next()), stamp).await
        })?;
        agent.set_version(Version::NEW.next());
        Ok(())
    }

    async fn update(&self, agent: &mut Agent) -> Result<(), RepoError> {
        stamp_update(agent, self.now());
        let s = agent.snapshot();
        let events = agent.take_events();
        let stamp = self.event_stamp();
        let version = in_transaction!(self, |tx| {
            let version = update_agent(tx, &s).await?;
            insert_events(tx, &events, Some(version), stamp).await?;
            Ok(version)
        })?;
        agent.set_version(version);
        Ok(())
    }

    async fn record_use(&self, id: AgentId, at: Timestamp) -> Result<(), RepoError> {
        let at = at.unix_micros();
        let mut conn = self.conn().await;
        conn.db()
            .execute_typed(
                &format!(
                    "UPDATE agents SET last_used_at = {0}
                     WHERE id = $1 AND (last_used_at IS NULL OR last_used_at < {0})",
                    ts(2)
                ),
                &[(&id.as_uuid(), Type::UUID), (&at, Type::INT8)],
            )
            .await
            .map_err(repo_error)?;
        Ok(())
    }
}

fn proposal_columns() -> String {
    format!(
        "id, version, agent_id, operation, request::text AS request, idempotency_key, \
         preview::text AS preview, digest, scopes, status, decided_by, {}, note, \
         response_status, response::text AS response, {}, {}",
        micros("decided_at"),
        micros("expires_at"),
        stamp_columns("")
    )
}

fn json_col(row: &Row, name: &str) -> Result<Option<serde_json::Value>, RepoError> {
    col::<Option<String>>(row, name)?
        .map(|text| serde_json::from_str(&text))
        .transpose()
        .map_err(|e| corrupt(format!("column {name:?}: {e}")))
}

fn proposal_from_row(row: &Row) -> Result<Proposal, RepoError> {
    let id = ProposalId::from_uuid(col(row, "id")?);
    let bad = |e: &dyn std::fmt::Display| corrupt(format!("proposal {id}: {e}"));
    let status: String = col(row, "status")?;
    let request = json_col(row, "request")?
        .and_then(ProposalRequest::from_json)
        .ok_or_else(|| bad(&"its request isn't {params, query, body}"))?;
    let response = match (
        col::<Option<i32>>(row, "response_status")?,
        json_col(row, "response")?,
    ) {
        (Some(status), Some(body)) => {
            let status = u16::try_from(status).map_err(|e| bad(&e))?;
            Some(ProposalResponse::new(status, body).map_err(|e| bad(&e))?)
        }
        _ => None,
    };
    Proposal::restore(ProposalSnapshot {
        id,
        version: version_from_db(col(row, "version")?)?,
        stamps: stamps_from_row(row)?,
        agent: AgentId::from_uuid(col(row, "agent_id")?),
        operation: OperationId::new(&col::<String>(row, "operation")?).map_err(|e| bad(&e))?,
        request,
        idempotency_key: col::<Option<String>>(row, "idempotency_key")?
            .map(|k| IdempotencyKey::new(&k))
            .transpose()
            .map_err(|e| bad(&e))?,
        preview: json_col(row, "preview")?.ok_or_else(|| bad(&"no preview"))?,
        digest: col(row, "digest")?,
        scopes: scopes_from_db(col(row, "scopes")?)?,
        status: ProposalStatus::parse(&status).map_err(|s| bad(&format!("status {s:?}")))?,
        decided_by: col::<Option<Uuid>>(row, "decided_by")?.map(UserId::from_uuid),
        decided_at: opt_ts(col(row, "decided_at")?),
        note: col(row, "note")?,
        response,
        expires_at: Timestamp::from_unix_micros(col(row, "expires_at")?),
    })
    .map_err(|e| bad(&e))
}

/// A proposal's response as `(status, body text)` columns.
fn response_columns(s: &ProposalSnapshot) -> (Option<i32>, Option<String>) {
    match &s.response {
        Some(r) => (Some(i32::from(r.status())), Some(r.body().to_string())),
        None => (None, None),
    }
}

/// `ProposalRepo::insert` inside the caller's transaction.
pub(crate) async fn insert_proposal(
    tx: &impl Queries,
    s: &ProposalSnapshot,
) -> Result<(), RepoError> {
    let (response_status, response) = response_columns(s);
    let decided_by = s.decided_by.map(|u| u.as_uuid());
    let key = s.idempotency_key.as_ref().map(|k| k.as_str().to_string());
    tx.query_typed(
        &format!(
            "INSERT INTO agent_proposals (id, version, agent_id, operation, request,
                 idempotency_key, preview, digest, scopes, status, decided_by, decided_at,
                 note, response_status, response, expires_at)
             VALUES ($1, 1, $2, $3, $4::text::jsonb, $5, $6::text::jsonb, $7, $8, $9, $10,
                 {}, $12, $13, $14::text::jsonb, {})",
            ts(11),
            ts(15)
        ),
        &[
            (&s.id.as_uuid(), Type::UUID),
            (&s.agent.as_uuid(), Type::UUID),
            (&s.operation.as_str(), Type::TEXT),
            (&s.request.to_json().to_string(), Type::TEXT),
            (&key, Type::TEXT),
            (&s.preview.to_string(), Type::TEXT),
            (&s.digest, Type::TEXT),
            (&scopes_to_db(&s.scopes), Type::TEXT_ARRAY),
            (&s.status.as_str(), Type::TEXT),
            (&decided_by, Type::UUID),
            (&opt_micros(s.decided_at), Type::INT8),
            (&s.note, Type::TEXT),
            (&response_status, Type::INT4),
            (&response, Type::TEXT),
            (&s.expires_at.unix_micros(), Type::INT8),
        ],
    )
    .await
    .map_err(repo_error)?;
    write_stamps(
        tx,
        "agent_proposals",
        "id",
        &s.id.as_uuid(),
        Type::UUID,
        s.stamps,
    )
    .await
}

/// `ProposalRepo::update` inside the caller's transaction; the new version. Writes what a
/// decision changes: the request, preview and scopes never do.
pub(crate) async fn update_proposal(
    tx: &impl Queries,
    s: &ProposalSnapshot,
) -> Result<Version, RepoError> {
    let (response_status, response) = response_columns(s);
    let decided_by = s.decided_by.map(|u| u.as_uuid());
    let row = tx
        .query_typed_opt(
            &format!(
                "UPDATE agent_proposals SET version = version + 1, status = $3,
                     decided_by = $4, decided_at = {}, note = $6, response_status = $7,
                     response = $8::text::jsonb
                 WHERE id = $1 AND version = $2 RETURNING version",
                ts(5)
            ),
            &[
                (&s.id.as_uuid(), Type::UUID),
                (&version_to_db(s.version)?, Type::INT8),
                (&s.status.as_str(), Type::TEXT),
                (&decided_by, Type::UUID),
                (&opt_micros(s.decided_at), Type::INT8),
                (&s.note, Type::TEXT),
                (&response_status, Type::INT4),
                (&response, Type::TEXT),
            ],
        )
        .await
        .map_err(repo_error)?
        .ok_or(RepoError::StaleVersion)?;
    let version = version_from_db(col(&row, "version")?)?;
    write_stamps(
        tx,
        "agent_proposals",
        "id",
        &s.id.as_uuid(),
        Type::UUID,
        s.stamps,
    )
    .await?;
    Ok(version)
}

impl<C: PgConnection> ProposalRepo for PgStore<C> {
    async fn get(&self, id: ProposalId) -> Result<Option<Proposal>, RepoError> {
        let mut conn = self.conn().await;
        let row = conn
            .db()
            .query_typed_opt(
                &format!(
                    "SELECT {} FROM agent_proposals WHERE id = $1",
                    proposal_columns()
                ),
                &[(&id.as_uuid(), Type::UUID)],
            )
            .await
            .map_err(repo_error)?;
        row.as_ref().map(proposal_from_row).transpose()
    }

    async fn find_by_key(
        &self,
        agent: AgentId,
        key: &IdempotencyKey,
    ) -> Result<Option<Proposal>, RepoError> {
        let mut conn = self.conn().await;
        // `agent_proposal_key` serves this.
        let row = conn
            .db()
            .query_typed_opt(
                &format!(
                    "SELECT {} FROM agent_proposals
                     WHERE agent_id = $1 AND idempotency_key = $2",
                    proposal_columns()
                ),
                &[(&agent.as_uuid(), Type::UUID), (&key.as_str(), Type::TEXT)],
            )
            .await
            .map_err(repo_error)?;
        row.as_ref().map(proposal_from_row).transpose()
    }

    async fn list(
        &self,
        filter: &ProposalFilter,
        page: PageRequest<ProposalId>,
    ) -> Result<Page<Proposal>, RepoError> {
        let agent = filter.agent.map(|a| a.as_uuid());
        let status = filter.status.map(ProposalStatus::as_str);
        let within = filter.within.as_deref().map(scopes_to_db);
        let after = page.after.map(|id| id.as_uuid());
        let fetch = overfetch(page.limit);
        let mut params: Vec<Param<'_>> = Vec::new();
        let mut conditions = Vec::new();
        if let Some(agent) = &agent {
            params.push((agent, Type::UUID));
            conditions.push(format!("agent_id = ${}", params.len()));
        }
        if let Some(status) = &status {
            params.push((status, Type::TEXT));
            conditions.push(format!("status = ${}", params.len()));
        }
        if let Some(within) = &within {
            params.push((within, Type::TEXT_ARRAY));
            conditions.push(format!("scopes <@ ${}", params.len()));
        }
        let rest = paged(conditions, &mut params, &after, &fetch);
        let mut conn = self.conn().await;
        // `agent_proposals_by_agent` or `_by_status` serve the filtered lists.
        let found = conn
            .db()
            .query_typed(
                &format!("SELECT {} FROM agent_proposals {rest}", proposal_columns()),
                &params,
            )
            .await
            .map_err(repo_error)?
            .iter()
            .map(proposal_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Page::from_overfetch(found, page.limit))
    }

    async fn due(&self, now: Timestamp, limit: u32) -> Result<Vec<Proposal>, RepoError> {
        let now = now.unix_micros();
        let limit = i64::from(limit);
        let mut conn = self.conn().await;
        // `agent_proposals_due` serves this.
        conn.db()
            .query_typed(
                &format!(
                    "SELECT {} FROM agent_proposals
                     WHERE status = 'pending' AND expires_at <= {}
                     ORDER BY expires_at, id LIMIT $2",
                    proposal_columns(),
                    ts(1)
                ),
                &[(&now, Type::INT8), (&limit, Type::INT8)],
            )
            .await
            .map_err(repo_error)?
            .iter()
            .map(proposal_from_row)
            .collect()
    }

    async fn insert(&self, proposal: &mut Proposal) -> Result<(), RepoError> {
        stamp_insert(proposal, self.now());
        let s = proposal.snapshot();
        let events = proposal.take_events();
        let stamp = self.event_stamp();
        in_transaction!(self, |tx| {
            insert_proposal(tx, &s).await?;
            insert_events(tx, &events, Some(Version::NEW.next()), stamp).await
        })?;
        proposal.set_version(Version::NEW.next());
        Ok(())
    }

    async fn update(&self, proposal: &mut Proposal) -> Result<(), RepoError> {
        stamp_update(proposal, self.now());
        let s = proposal.snapshot();
        let events = proposal.take_events();
        let stamp = self.event_stamp();
        let version = in_transaction!(self, |tx| {
            let version = update_proposal(tx, &s).await?;
            insert_events(tx, &events, Some(version), stamp).await?;
            Ok(version)
        })?;
        proposal.set_version(version);
        Ok(())
    }
}

fn call_from_row(row: &Row) -> Result<AgentCall, RepoError> {
    let id = AgentCallId::from_uuid(col(row, "id")?);
    let bad = |e: &dyn std::fmt::Display| corrupt(format!("agent call {id}: {e}"));
    let mode: String = col(row, "mode")?;
    let status: i32 = col(row, "status")?;
    let duration: i64 = col(row, "duration_ms")?;
    AgentCall::new(
        id,
        NewAgentCall {
            agent: AgentId::from_uuid(col(row, "agent_id")?),
            operation: OperationId::new(&col::<String>(row, "operation")?).map_err(|e| bad(&e))?,
            mode: CallMode::parse(&mode).map_err(|m| bad(&format!("mode {m:?}")))?,
            status: u16::try_from(status).map_err(|e| bad(&e))?,
            error_code: col(row, "error_code")?,
            duration_ms: u32::try_from(duration).map_err(|e| bad(&e))?,
            at: Timestamp::from_unix_micros(col(row, "created_at")?),
        },
    )
    .map_err(|e| bad(&e))
}

impl<C: PgConnection> AgentCallRepo for PgStore<C> {
    async fn insert(&self, call: &AgentCall) -> Result<(), RepoError> {
        let status = i32::from(call.status());
        let duration = i64::from(call.duration_ms());
        let at = call.created_at().unix_micros();
        let mut conn = self.conn().await;
        conn.db()
            .execute_typed(
                &format!(
                    "INSERT INTO agent_calls (id, agent_id, operation, mode, status, error_code,
                         duration_ms, created_at)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, {})",
                    ts(8)
                ),
                &[
                    (&call.id().as_uuid(), Type::UUID),
                    (&call.agent().as_uuid(), Type::UUID),
                    (&call.operation().as_str(), Type::TEXT),
                    (&call.mode().as_str(), Type::TEXT),
                    (&status, Type::INT4),
                    (&call.error_code(), Type::TEXT),
                    (&duration, Type::INT8),
                    (&at, Type::INT8),
                ],
            )
            .await
            .map_err(repo_error)?;
        Ok(())
    }

    async fn list_for_agent(
        &self,
        agent: AgentId,
        page: PageRequest<AgentCallId>,
    ) -> Result<Page<AgentCall>, RepoError> {
        let agent = agent.as_uuid();
        let after = page.after.map(|id| id.as_uuid());
        let fetch = overfetch(page.limit);
        let mut params: Vec<Param<'_>> = vec![(&agent, Type::UUID)];
        let rest = paged(
            vec!["agent_id = $1".to_string()],
            &mut params,
            &after,
            &fetch,
        );
        let mut conn = self.conn().await;
        // `agent_calls_by_agent` serves this.
        let found = conn
            .db()
            .query_typed(
                &format!(
                    "SELECT id, agent_id, operation, mode, status, error_code, duration_ms, {}
                     FROM agent_calls {rest}",
                    micros("created_at")
                ),
                &params,
            )
            .await
            .map_err(repo_error)?
            .iter()
            .map(call_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Page::from_overfetch(found, page.limit))
    }

    async fn prune_before(&self, before: Timestamp, limit: u32) -> Result<u32, RepoError> {
        let before = before.unix_micros();
        let limit = i64::from(limit);
        let mut conn = self.conn().await;
        // `agent_calls_created_at` serves the inner scan.
        let deleted = conn
            .db()
            .execute_typed(
                &format!(
                    "DELETE FROM agent_calls WHERE id IN (
                         SELECT id FROM agent_calls WHERE created_at < {} LIMIT $2
                     )",
                    ts(1)
                ),
                &[(&before, Type::INT8), (&limit, Type::INT8)],
            )
            .await
            .map_err(repo_error)?;
        u32::try_from(deleted).map_err(corrupt)
    }
}
