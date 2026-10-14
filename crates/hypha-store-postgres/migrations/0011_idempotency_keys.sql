-- Idempotency keys (hypha_core::idempotency, design note 0007 O7). One row per actor
-- and key; the constraint name is part of the adapter contract (error.rs maps it to
-- `hypha_core::idempotency::keys::IDEMPOTENCY_KEY`). Rows are only inserted, in the
-- same transaction as the write they belong to.

CREATE TABLE idempotency_keys (
    -- 'system' has no ID; it is stored as the nil UUID so the key can be a primary key.
    actor_kind  text        NOT NULL CHECK (actor_kind IN ('system', 'user', 'agent')),
    actor_id    uuid        NOT NULL,
    key         text        NOT NULL CHECK (length(key) BETWEEN 1 AND 255),
    operation   text        NOT NULL,
    fingerprint bytea       NOT NULL CHECK (octet_length(fingerprint) = 32),
    result      uuid        NOT NULL,
    created_at  timestamptz NOT NULL,
    CONSTRAINT idempotency_key PRIMARY KEY (actor_kind, actor_id, key)
);

-- For the cleanup job that will drop old records.
CREATE INDEX idempotency_keys_created_at ON idempotency_keys (created_at);
