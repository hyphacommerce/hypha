-- External apps (design note 0016, section 8): one record per app Hypha signs calls to,
-- owning its name, whether it's enabled, its signing secret, its webhook and the events
-- it subscribes to. Shipping's rate providers were the first apps: their name, enabled
-- flag and secret move here, and their rate settings stay, linked to the app.
CREATE TABLE apps (
    key                text        CONSTRAINT app_key PRIMARY KEY
                                   CHECK (key ~ '^app_[a-z][a-z0-9_]*$' AND length(key) <= 64),
    version            bigint      NOT NULL CHECK (version > 0),
    name               text        NOT NULL,
    enabled            boolean     NOT NULL,
    -- Sealed under HYPHA_SECRET_KEY with the key as context, as shipping sealed it, so
    -- the providers' secrets carry over as they are.
    secret             bytea       NOT NULL,
    webhook_url        text,
    webhook_timeout_ms integer     CHECK (webhook_timeout_ms BETWEEN 1000 AND 30000),
    subscriptions      text[]      NOT NULL DEFAULT '{}',
    created_at         timestamptz NOT NULL DEFAULT now(),
    updated_at         timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT app_webhook CHECK ((webhook_url IS NULL) = (webhook_timeout_ms IS NULL))
);

INSERT INTO apps (key, version, name, enabled, secret, created_at, updated_at)
SELECT key, 1, name, enabled, secret, created_at, updated_at FROM shipping_app_providers;

ALTER TABLE shipping_app_providers
    DROP COLUMN name,
    DROP COLUMN enabled,
    DROP COLUMN secret,
    -- A provider is an app's rate settings: it needs its app and goes with it.
    ADD CONSTRAINT shipping_provider_app FOREIGN KEY (key) REFERENCES apps (key)
        ON DELETE CASCADE;
