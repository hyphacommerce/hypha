-- Extension calls (the hypha-extensions crate, design note 0029, §15): one row per call
-- Hypha made to an app's extension point (`shipping.rates`, `payments.start`, …), for
-- staff to see per app. Written once, never updated, so no version; pruned by age like
-- deliveries (`jobs.prune.extension_calls_after`).

-- No foreign key to apps: a call is recorded as it ended even if its app was deleted
-- meanwhile, and the log goes by age. `point` is text so the log keeps points a later
-- build retired. `correlation` is the prefixed ID of the cart, order or payment the
-- call was for.
CREATE TABLE extension_calls (
    id          uuid        CONSTRAINT extension_calls_pkey PRIMARY KEY,
    app         text        NOT NULL,
    point       text        NOT NULL,
    version     integer     NOT NULL CHECK (version BETWEEN 1 AND 65535),
    started_at  timestamptz NOT NULL,
    duration_ms bigint      NOT NULL CHECK (duration_ms >= 0),
    outcome     text        NOT NULL CHECK (outcome IN ('ok', 'failed', 'timeout', 'invalid')),
    status      integer     CHECK (status BETWEEN 100 AND 999),
    error       text,
    correlation text
);

-- An app's calls, newest first (IDs are UUIDv7, so by start).
CREATE INDEX extension_calls_by_app ON extension_calls (app, id DESC);
-- The prune job's scan.
CREATE INDEX extension_calls_started_at ON extension_calls (started_at);
