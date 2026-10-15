-- Domain events (design note 0016, section 3): written in the same transaction as the
-- change that recorded them, so an event exists if and only if its change committed.
--
-- `seq` orders rows by insertion, not commit: a transaction that commits late can add
-- rows below ones already read. The dispatcher therefore claims rows that aren't fanned
-- out yet (`FOR UPDATE SKIP LOCKED`) rather than reading after a cursor (EV6).
CREATE TABLE outbox_events (
    seq             bigint      GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    id              uuid        NOT NULL DEFAULT gen_random_uuid(),
    type            text        NOT NULL CHECK (type ~ '^[a-z][a-z_]*(\.[a-z][a-z_]*)+$'),
    version         smallint    NOT NULL CHECK (version >= 1),
    subject_type    text        NOT NULL,
    subject_id      uuid        NOT NULL,
    subject_version bigint      CHECK (subject_version >= 1),
    actor_kind      text        NOT NULL,
    actor_id        uuid        NOT NULL,
    data            jsonb       NOT NULL,
    -- The writing transaction's time (`now()` is fixed for a whole transaction).
    occurred_at     timestamptz NOT NULL DEFAULT now(),
    fanned_out_at   timestamptz,
    CONSTRAINT outbox_event_id UNIQUE (id)
);

-- What the dispatcher claims next: only rows not fanned out yet, in insertion order.
CREATE INDEX outbox_events_pending ON outbox_events (seq) WHERE fanned_out_at IS NULL;
-- Debugging an aggregate's history.
CREATE INDEX outbox_events_subject ON outbox_events (subject_type, subject_id, seq);
