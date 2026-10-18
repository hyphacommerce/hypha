-- Where apps answer extension points (design note 0029, §3, "Registering"), and the
-- store's search app (§3.1, `search.query`). Numbered 51: 50 is the OAuth tables' (§15).

-- One row per point an app answers. Set by staff for a private app; from the manifest
-- for an installed one. An app's endpoints go with it.
CREATE TABLE app_extension_endpoints (
    app     text    NOT NULL
        CONSTRAINT extension_endpoint_app REFERENCES apps (key) ON DELETE CASCADE,
    point   text    NOT NULL,
    version integer NOT NULL CHECK (version BETWEEN 1 AND 65535),
    url     text    NOT NULL,
    CONSTRAINT app_extension_endpoints_pkey PRIMARY KEY (app, point)
);

-- The app that answers `/storefront/search` (`search.query`); NULL for the built-in
-- engine. Not a foreign key: a search app that's gone falls back to the built-in engine,
-- like one that's down.
ALTER TABLE search_settings ADD COLUMN search_app text;
