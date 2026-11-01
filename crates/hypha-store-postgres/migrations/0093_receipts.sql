-- Receipts: what arrived against a purchase order, at one location. Written in the
-- order's transaction with the stock they brought.
CREATE TABLE receipts (
    id                uuid        CONSTRAINT receipts_pkey PRIMARY KEY,
    version           bigint      NOT NULL CHECK (version > 0),
    -- Only ordered orders get receipts, and only drafts are deleted, so this never
    -- blocks a delete.
    purchase_order_id uuid        NOT NULL
                                  CONSTRAINT receipt_purchase_order
                                  REFERENCES purchase_orders (id),
    -- No key, as for stock movements: locations are never deleted.
    location_id       uuid        NOT NULL,
    -- The order's rate when received: one unit of its currency in the store's, in
    -- 1/100,000,000.
    exchange_rate     bigint      CHECK (exchange_rate > 0),
    note              text,
    cancelled_at      timestamptz,
    created_at        timestamptz NOT NULL DEFAULT now(),
    updated_at        timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX receipts_by_purchase_order ON receipts (purchase_order_id, id);

CREATE TRIGGER hypha_note_write AFTER INSERT OR UPDATE OR DELETE ON receipts
    FOR EACH ROW WHEN (current_setting('hypha.log_writes', true) = 'on')
    EXECUTE FUNCTION hypha_note_write('id');

-- A receipt's lines, one per variant, written once with it.
CREATE TABLE receipt_lines (
    receipt_id             uuid    NOT NULL REFERENCES receipts (id) ON DELETE CASCADE,
    position               integer NOT NULL,
    variant_id             uuid    NOT NULL,
    accepted               integer NOT NULL CHECK (accepted >= 0),
    rejected               integer NOT NULL CHECK (rejected >= 0),
    -- In the store's currency, in 1/10,000 of its minor unit; both or neither.
    landed_unit_cost       bigint  CHECK (landed_unit_cost >= 0),
    landed_unit_cost_currency text,
    CHECK (accepted + rejected > 0),
    CHECK ((landed_unit_cost IS NULL) = (landed_unit_cost_currency IS NULL)),
    CONSTRAINT receipt_lines_pkey PRIMARY KEY (receipt_id, variant_id)
);
