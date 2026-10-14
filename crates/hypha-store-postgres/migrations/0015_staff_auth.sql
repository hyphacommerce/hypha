-- Staff users, roles, invites, and how callers authenticate (the hypha-auth and
-- hypha-users crates, design note 0008). Constraint and unique index names are part of
-- the adapter contract: error.rs maps them to the crates' `keys`.
--
-- No foreign keys between these tables: each is its own aggregate (like orders and
-- carts), and the domain checks references where it matters.
-- Tokens (sessions, keys, invites, resets) are stored only as SHA-256 hashes.

-- An identity and its credentials. A credential is a kind, an optional lookup a login
-- finds it by (a password's email), and JSON data, so a new login method needs no new
-- column (0008, A2). `principal_kind` is copied onto each credential for the login key.
CREATE TABLE identities (
    id             uuid   CONSTRAINT identities_pkey PRIMARY KEY,
    version        bigint NOT NULL CHECK (version > 0),
    principal_kind text   NOT NULL CHECK (principal_kind IN ('staff')),
    principal_id   uuid   NOT NULL,
    CONSTRAINT identity_principal UNIQUE (principal_kind, principal_id)
);

CREATE TABLE credentials (
    identity_id    uuid  NOT NULL REFERENCES identities (id) ON DELETE CASCADE,
    principal_kind text  NOT NULL,
    kind           text  NOT NULL,
    lookup         text,
    data           jsonb NOT NULL,
    PRIMARY KEY (identity_id, kind),
    -- NULL lookups (second factors) never collide.
    CONSTRAINT identity_login UNIQUE (principal_kind, kind, lookup)
);

CREATE TABLE sessions (
    id              uuid        CONSTRAINT sessions_pkey PRIMARY KEY,
    version         bigint      NOT NULL CHECK (version > 0),
    identity_id     uuid        NOT NULL,
    principal_kind  text        NOT NULL CHECK (principal_kind IN ('staff')),
    principal_id    uuid        NOT NULL,
    token_hash      bytea       NOT NULL CONSTRAINT session_token UNIQUE
                                CHECK (octet_length(token_hash) = 32),
    state           text        NOT NULL CHECK (state IN ('pending', 'active')),
    failed_attempts integer     NOT NULL CHECK (failed_attempts >= 0),
    mfa_at          timestamptz,
    created_at      timestamptz NOT NULL,
    expires_at      timestamptz NOT NULL,
    revoked_at      timestamptz
);

-- `revoke_sessions` (password change or reset) finds an identity's live sessions.
CREATE INDEX sessions_live_by_identity ON sessions (identity_id) WHERE revoked_at IS NULL;

CREATE TABLE api_keys (
    id              uuid        CONSTRAINT api_keys_pkey PRIMARY KEY,
    version         bigint      NOT NULL CHECK (version > 0),
    kind            text        NOT NULL CHECK (kind IN ('secret', 'publishable')),
    name            text        NOT NULL,
    token_hash      bytea       NOT NULL CONSTRAINT api_key_token UNIQUE
                                CHECK (octet_length(token_hash) = 32),
    last_four       text        NOT NULL,
    scopes          text[]      NOT NULL,
    created_by_kind text        NOT NULL,
    created_by_id   uuid        NOT NULL,
    created_at      timestamptz NOT NULL,
    revoked_at      timestamptz
);

CREATE TABLE password_resets (
    id          uuid        CONSTRAINT password_resets_pkey PRIMARY KEY,
    version     bigint      NOT NULL CHECK (version > 0),
    identity_id uuid        NOT NULL,
    token_hash  bytea       NOT NULL CONSTRAINT password_reset_token UNIQUE
                            CHECK (octet_length(token_hash) = 32),
    created_at  timestamptz NOT NULL,
    expires_at  timestamptz NOT NULL,
    used_at     timestamptz
);

-- Roles: the owner role has every scope (`scopes` empty), so it never needs a migration
-- when a scope is added.
CREATE TABLE roles (
    id       uuid    CONSTRAINT roles_pkey PRIMARY KEY,
    version  bigint  NOT NULL CHECK (version > 0),
    name     text    NOT NULL,
    is_owner boolean NOT NULL,
    scopes   text[]  NOT NULL,
    CHECK (NOT is_owner OR cardinality(scopes) = 0)
);

CREATE UNIQUE INDEX role_name ON roles (lower(name));
CREATE UNIQUE INDEX role_owner ON roles (is_owner) WHERE is_owner;

CREATE TABLE users (
    id         uuid        CONSTRAINT users_pkey PRIMARY KEY,
    version    bigint      NOT NULL CHECK (version > 0),
    -- Email::new lowercases, so a plain unique key is case-insensitive.
    email      text        NOT NULL CONSTRAINT user_email UNIQUE CHECK (email = lower(email)),
    name       text        NOT NULL,
    role_id    uuid        NOT NULL,
    status     text        NOT NULL CHECK (status IN ('active', 'disabled')),
    created_at timestamptz NOT NULL
);

CREATE INDEX users_by_role ON users (role_id);

CREATE TABLE invites (
    id              uuid        CONSTRAINT invites_pkey PRIMARY KEY,
    version         bigint      NOT NULL CHECK (version > 0),
    email           text        NOT NULL CONSTRAINT invite_email UNIQUE
                                CHECK (email = lower(email)),
    role_id         uuid        NOT NULL,
    token_hash      bytea       NOT NULL CONSTRAINT invite_token UNIQUE
                                CHECK (octet_length(token_hash) = 32),
    invited_by_kind text        NOT NULL,
    invited_by_id   uuid        NOT NULL,
    created_at      timestamptz NOT NULL,
    expires_at      timestamptz NOT NULL,
    accepted_at     timestamptz
);

-- API keys are actors now (`Actor::ApiKey`), and they take idempotency keys too.
ALTER TABLE idempotency_keys
    DROP CONSTRAINT idempotency_keys_actor_kind_check,
    ADD CONSTRAINT idempotency_actor_kind
        CHECK (actor_kind IN ('system', 'user', 'agent', 'api_key'));
