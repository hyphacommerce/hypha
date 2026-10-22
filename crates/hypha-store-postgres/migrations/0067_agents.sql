-- Admin agents (the hypha-agents crate): principals of their own that act on the admin
-- surface through MCP, the writes they propose for staff to approve, and a log of every
-- tool call they made.

-- An agent. A connected one acts for a staff member (`user_id`, which is its kind); no
-- foreign key to users, as for other actors: staff are deactivated, not deleted. The
-- grant is in columns: read scopes, write scopes by mode, personal data scopes and the
-- optional operations allowlist (NULL: none). `last_used_at` is written apart from the
-- version (`AgentRepo::record_use`).
CREATE TABLE agents (
    id             uuid        CONSTRAINT agents_pkey PRIMARY KEY,
    version        bigint      NOT NULL,
    name           text        NOT NULL,
    user_id        uuid,
    client_id      text,
    client_name    text,
    read_scopes    text[]      NOT NULL,
    propose_scopes text[]      NOT NULL,
    apply_scopes   text[]      NOT NULL,
    customer_data  text[]      NOT NULL,
    operations     text[],
    enabled        boolean     NOT NULL,
    last_used_at   timestamptz,
    revoked_at     timestamptz,
    created_at     timestamptz NOT NULL DEFAULT now(),
    updated_at     timestamptz NOT NULL DEFAULT now(),
    CHECK ((client_id IS NULL) = (client_name IS NULL))
);
-- A staff member's connected agents, newest first.
CREATE INDEX agents_by_user ON agents (user_id, id DESC) WHERE user_id IS NOT NULL;

-- A write an agent previewed and left for a person. `request` is what replays it
-- (`{"params", "query", "body"}`), `preview` the host's preview as it was, `digest` what
-- applying must match. The decision columns are set together once it leaves `pending`.
CREATE TABLE agent_proposals (
    id              uuid        CONSTRAINT agent_proposals_pkey PRIMARY KEY,
    version         bigint      NOT NULL,
    agent_id        uuid        NOT NULL REFERENCES agents (id) ON DELETE CASCADE,
    operation       text        NOT NULL,
    request         jsonb       NOT NULL,
    idempotency_key text,
    preview         jsonb       NOT NULL,
    digest          text        NOT NULL,
    scopes          text[]      NOT NULL,
    status          text        NOT NULL
        CHECK (status IN ('pending', 'applied', 'rejected', 'expired', 'failed')),
    decided_by      uuid,
    decided_at      timestamptz,
    note            text,
    response_status integer     CHECK (response_status BETWEEN 100 AND 599),
    response        jsonb,
    expires_at      timestamptz NOT NULL,
    created_at      timestamptz NOT NULL DEFAULT now(),
    updated_at      timestamptz NOT NULL DEFAULT now(),
    CHECK ((status = 'pending') = (decided_at IS NULL)),
    CHECK ((response_status IS NULL) = (response IS NULL))
);
-- One proposal per agent and key: an agent's retry finds the first.
CREATE UNIQUE INDEX agent_proposal_key ON agent_proposals (agent_id, idempotency_key)
    WHERE idempotency_key IS NOT NULL;
-- An agent's proposals, newest first.
CREATE INDEX agent_proposals_by_agent ON agent_proposals (agent_id, id DESC);
-- The inbox: proposals by status, newest first.
CREATE INDEX agent_proposals_by_status ON agent_proposals (status, id DESC);
-- The expiry job's scan.
CREATE INDEX agent_proposals_due ON agent_proposals (expires_at) WHERE status = 'pending';

-- One row per tool call an agent made, reads included. Written once, never updated, so no
-- version; pruned by age (`jobs.prune.agent_calls_after`). No foreign key, like
-- `extension_calls`: the log goes by age. No bodies: they may hold personal data.
CREATE TABLE agent_calls (
    id          uuid        CONSTRAINT agent_calls_pkey PRIMARY KEY,
    agent_id    uuid        NOT NULL,
    operation   text        NOT NULL,
    mode        text        NOT NULL CHECK (mode IN ('read', 'preview', 'propose', 'apply')),
    status      integer     NOT NULL CHECK (status BETWEEN 100 AND 599),
    error_code  text,
    duration_ms bigint      NOT NULL CHECK (duration_ms >= 0),
    created_at  timestamptz NOT NULL
);
-- An agent's calls, newest first (IDs are UUIDv7, so by time).
CREATE INDEX agent_calls_by_agent ON agent_calls (agent_id, id DESC);
-- The prune job's scan.
CREATE INDEX agent_calls_created_at ON agent_calls (created_at);
