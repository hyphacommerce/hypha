-- Applying exactly what was previewed: a preview's token, stored as its SHA-256 like every
-- token. Bound to who previewed (`actor_*`) and the request (`request`, the host's
-- fingerprint of method, path, query and body); `ids` are the IDs the preview was handed,
-- in order, `digest` what its events came to. `used_at` is set in the transaction of the
-- write that applied it. Pruned once expired.
CREATE TABLE preview_tokens (
    hash       bytea       CONSTRAINT preview_token PRIMARY KEY
                           CHECK (octet_length(hash) = 32),
    actor_kind text        NOT NULL,
    actor_id   uuid        NOT NULL,
    request    bytea       NOT NULL CHECK (octet_length(request) = 32),
    ids        uuid[]      NOT NULL,
    digest     text        NOT NULL,
    expires_at timestamptz NOT NULL,
    used_at    timestamptz
);
-- The prune job's scan.
CREATE INDEX preview_tokens_expires_at ON preview_tokens (expires_at);

-- Autonomous agents sign in with a key: their identity's principal is an `agent`.
ALTER TABLE identities DROP CONSTRAINT identities_principal_kind_check;
ALTER TABLE identities
    ADD CONSTRAINT identities_principal_kind_check
    CHECK (principal_kind IN ('staff', 'customer', 'agent'));
