-- Transfers: stock moved between two of the store's locations, with a time in transit.
-- Numbers come from their own sequence (atomic, never rolled back), shown `TR-1001`.
CREATE SEQUENCE transfer_numbers START 1001;

CREATE TABLE transfers (
    id             uuid        CONSTRAINT transfers_pkey PRIMARY KEY,
    version        bigint      NOT NULL CHECK (version > 0),
    number         bigint      NOT NULL CHECK (number > 0)
                               CONSTRAINT transfer_number UNIQUE,
    -- No keys, as for stock movements: locations are never deleted.
    origin_id      uuid        NOT NULL,
    destination_id uuid        NOT NULL CHECK (destination_id <> origin_id),
    expected_at    timestamptz,
    reference      text,
    note           text,
    status         text        NOT NULL
                               CHECK (status IN ('draft', 'in_transit', 'received', 'cancelled')),
    shipped_at     timestamptz,
    received_at    timestamptz,
    cancelled_at   timestamptz,
    -- What lists show without the lines: written with the transfer.
    line_count     integer     NOT NULL DEFAULT 0,
    units          bigint      NOT NULL DEFAULT 0,
    created_at     timestamptz NOT NULL DEFAULT now(),
    updated_at     timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX transfers_by_destination ON transfers (destination_id, status);

CREATE TRIGGER hypha_note_write AFTER INSERT OR UPDATE OR DELETE ON transfers
    FOR EACH ROW WHEN (current_setting('hypha.log_writes', true) = 'on')
    EXECUTE FUNCTION hypha_note_write('id');

-- A transfer's lines, one per variant, written whole with it. No key into variants: a
-- removed variant's lines stay.
CREATE TABLE transfer_lines (
    transfer_id uuid    NOT NULL REFERENCES transfers (id) ON DELETE CASCADE,
    position    integer NOT NULL,
    variant_id  uuid    NOT NULL,
    quantity    integer NOT NULL CHECK (quantity > 0),
    shipped     integer NOT NULL DEFAULT 0 CHECK (shipped >= 0),
    received    integer NOT NULL DEFAULT 0 CHECK (received >= 0),
    rejected    integer NOT NULL DEFAULT 0 CHECK (rejected >= 0),
    CONSTRAINT transfer_lines_pkey PRIMARY KEY (transfer_id, variant_id),
    CHECK (received + rejected <= shipped)
);

CREATE INDEX transfer_lines_by_variant ON transfer_lines (variant_id);
