-- Deliveries of outbox events (design note 0016, section 5): one row per event and
-- destination ("handler:variant_cleanup", later "app:app_erp"), with its attempts. The
-- dispatcher leases due rows in a short transaction, runs them with no connection held,
-- and records the outcomes in another.
CREATE TABLE event_deliveries (
    id              uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    event_seq       bigint      NOT NULL REFERENCES outbox_events (seq) ON DELETE CASCADE,
    destination     text        NOT NULL CHECK (destination ~ '^(handler|app):[a-z0-9_]+$'),
    status          text        NOT NULL CHECK (status IN ('pending', 'succeeded', 'failed')),
    attempts        integer     NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    next_attempt_at timestamptz NOT NULL,
    leased_until    timestamptz,
    last_error      text,
    -- Fanning out twice (a claim that ran again) adds nothing.
    CONSTRAINT event_delivery_destination UNIQUE (event_seq, destination)
);

-- What `lease_due` scans: pending rows by when they're due.
CREATE INDEX event_deliveries_due ON event_deliveries (next_attempt_at)
    WHERE status = 'pending';

-- Pruning old events goes by their time.
CREATE INDEX outbox_events_occurred ON outbox_events (occurred_at);
