-- The stock ledger: one row per change to a count, written in the transaction of the
-- item that changed, like `outbox_events`. Kept for good: stock value and the cost of
-- goods sold are read from it. No foreign keys: an item's rows outlive it and name its
-- variant.
--
-- `position` orders rows by insertion. An item's rows are inserted after its row is
-- locked, so they're in the order its writes committed.
CREATE TABLE stock_movements (
    position         bigint      GENERATED ALWAYS AS IDENTITY
                                 CONSTRAINT stock_movements_pkey PRIMARY KEY,
    item_id          uuid        NOT NULL,
    variant_id       uuid        NOT NULL,
    -- None for a change that moved no units (a cost).
    location_id      uuid,
    -- Which count moved: only `on_hand` so far.
    quantity         text        NOT NULL DEFAULT 'on_hand' CHECK (quantity = 'on_hand'),
    delta            bigint      NOT NULL,
    after            bigint,
    reason           text        NOT NULL,
    -- What caused it: `manual`, `fulfillment` (parent: the order), `receipt` (parent: the
    -- purchase order), `purchase_order`, `transfer`, `migration`.
    source_kind      text        NOT NULL,
    source_id        uuid,
    source_parent_id uuid,
    actor_kind       text        NOT NULL,
    actor_id         uuid        NOT NULL,
    note             text,
    -- The writing transaction's time, as for events.
    at               timestamptz NOT NULL DEFAULT now(),
    CHECK ((location_id IS NULL) = (after IS NULL))
);

CREATE INDEX stock_movements_item ON stock_movements (item_id, position);

-- What every level holds now, so each location's deltas add up to its count from the
-- start.
INSERT INTO stock_movements (item_id, variant_id, location_id, delta, after, reason,
                             source_kind, actor_kind, actor_id)
SELECT l.item_id, i.variant_id, l.location_id, l.on_hand, l.on_hand, 'opening', 'migration',
       'system', '00000000-0000-0000-0000-000000000000'
FROM inventory_levels l JOIN inventory_items i ON i.id = l.item_id
WHERE l.on_hand <> 0
ORDER BY l.item_id, l.position;
