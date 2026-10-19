-- Spores' settings (design note 0030, §8): what the merchant sets for a spore built into
-- the API, one row per spore, its values as JSON (secret fields sealed under
-- HYPHA_SECRET_KEY, `{"sealed": "<hex>"}`). No foreign key to `apps`: a spore's row is
-- made at startup and never deleted, and its settings outlive a build without it. Named
-- outside `spore_*`, which belongs to spores' own tables (their migration check).
CREATE TABLE built_in_settings (
    key        text        CONSTRAINT built_in_settings_key PRIMARY KEY
                           CHECK (key ~ '^spore_[a-z][a-z0-9_]*$' AND length(key) <= 64),
    version    bigint      NOT NULL CHECK (version > 0),
    settings   jsonb       NOT NULL CHECK (jsonb_typeof(settings) = 'object'),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);
