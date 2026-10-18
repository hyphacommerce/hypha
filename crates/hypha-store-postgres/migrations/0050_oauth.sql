-- Installing apps through OAuth (design note 0029, AP5, §15): an app's identity as an
-- actor, the manifest it was installed from, the grant staff approved, pending
-- authorization requests, codes and token pairs (stored as SHA-256 only).

-- `Actor::App(AppId)`: a UUID beside the key. Existing apps (made by hand) get one.
ALTER TABLE apps
    ADD COLUMN id                  uuid,
    ADD COLUMN manifest_url        text,
    ADD COLUMN manifest            jsonb,
    ADD COLUMN manifest_fetched_at timestamptz,
    -- From the manifest, for listing without parsing it.
    ADD COLUMN publisher           text,
    ADD COLUMN privacy_url         text,
    -- Set when uninstalled: the app then waits for its last webhook (`app.uninstalled`).
    ADD COLUMN uninstalled_at      timestamptz;
UPDATE apps SET id = gen_random_uuid();
ALTER TABLE apps
    ALTER COLUMN id SET NOT NULL,
    ADD CONSTRAINT app_id UNIQUE (id),
    ADD CONSTRAINT app_manifest CHECK (
        (manifest IS NULL) = (manifest_url IS NULL)
        AND (manifest IS NULL) = (manifest_fetched_at IS NULL)
    );

-- The scopes staff approved for an app: one grant per app, gone with it.
CREATE TABLE app_grants (
    app_key          text        CONSTRAINT app_grant_key PRIMARY KEY
                                 REFERENCES apps (key) ON DELETE CASCADE,
    scopes           text[]      NOT NULL,
    approved_by_kind text        NOT NULL,
    approved_by_id   uuid        NOT NULL,
    approved_at      timestamptz NOT NULL
);

-- What an app asked for at /oauth/authorize, answered on the consent page; kept as the
-- app's history. Not tied to `apps`: a first install's app doesn't exist yet.
CREATE TABLE oauth_requests (
    id               uuid        CONSTRAINT oauth_request_id PRIMARY KEY,
    version          bigint      NOT NULL CHECK (version > 0),
    app_key          text        NOT NULL,
    client_id        text        NOT NULL,
    manifest         jsonb       NOT NULL,
    redirect_uri     text        NOT NULL,
    state            text,
    code_challenge   text        NOT NULL,
    scopes           text[]      NOT NULL,
    previous_scopes  text[],
    status           text        NOT NULL CHECK (status IN ('pending', 'approved', 'denied')),
    decided_by_kind  text,
    decided_by_id    uuid,
    decided_at       timestamptz,
    created_at       timestamptz NOT NULL,
    expires_at       timestamptz NOT NULL,
    CONSTRAINT oauth_request_decision CHECK (
        (status = 'pending') = (decided_at IS NULL)
        AND (decided_at IS NULL) = (decided_by_kind IS NULL)
    )
);
CREATE INDEX oauth_requests_app ON oauth_requests (app_key, created_at DESC, id DESC);

-- Authorization codes (`hypha_ac_…`), by SHA-256: 60 seconds, once.
CREATE TABLE oauth_codes (
    hash           bytea       CONSTRAINT oauth_code PRIMARY KEY CHECK (length(hash) = 32),
    version        bigint      NOT NULL CHECK (version > 0),
    request_id     uuid        NOT NULL REFERENCES oauth_requests (id) ON DELETE CASCADE,
    app_key        text        NOT NULL,
    client_id      text        NOT NULL,
    redirect_uri   text        NOT NULL,
    code_challenge text        NOT NULL,
    scopes         text[]      NOT NULL,
    first_install  boolean     NOT NULL,
    family         uuid        NOT NULL,
    expires_at     timestamptz NOT NULL,
    used_at        timestamptz
);

-- Token pairs: an access token (`hypha_at_…`, an hour) and its refresh token
-- (`hypha_rt_…`, 30 days, rotating), by SHA-256. A family is every pair since one code
-- exchange: reusing a rotated refresh token revokes the app's tokens.
CREATE TABLE oauth_refresh_tokens (
    id                 uuid        CONSTRAINT oauth_token_id PRIMARY KEY,
    version            bigint      NOT NULL CHECK (version > 0),
    family             uuid        NOT NULL,
    app_key            text        NOT NULL REFERENCES apps (key) ON DELETE CASCADE,
    scopes             text[]      NOT NULL,
    access_hash        bytea       NOT NULL CHECK (length(access_hash) = 32),
    access_expires_at  timestamptz NOT NULL,
    refresh_hash       bytea       NOT NULL CHECK (length(refresh_hash) = 32),
    refresh_expires_at timestamptz NOT NULL,
    created_at         timestamptz NOT NULL,
    rotated_at         timestamptz,
    revoked_at         timestamptz,
    CONSTRAINT oauth_access_token UNIQUE (access_hash),
    CONSTRAINT oauth_refresh_token UNIQUE (refresh_hash)
);
CREATE INDEX oauth_refresh_tokens_family ON oauth_refresh_tokens (family);
CREATE INDEX oauth_refresh_tokens_app ON oauth_refresh_tokens (app_key);

-- Apps are actors now (`Actor::App`), and their writes take idempotency keys too.
ALTER TABLE idempotency_keys
    DROP CONSTRAINT idempotency_actor_kind,
    ADD CONSTRAINT idempotency_actor_kind
        CHECK (actor_kind IN ('system', 'user', 'agent', 'api_key', 'app'));
