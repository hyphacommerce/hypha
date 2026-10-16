-- Fulfilment and cancellation on orders, and retired inventory items (design note 0018).

-- What each line's standing fulfilments shipped; the order's status follows from it.
ALTER TABLE order_lines
    ADD COLUMN fulfilled_quantity bigint NOT NULL DEFAULT 0
        CHECK (fulfilled_quantity BETWEEN 0 AND quantity);

ALTER TABLE orders DROP CONSTRAINT orders_fulfillment_status_check;
ALTER TABLE orders ADD CONSTRAINT orders_fulfillment_status_check
    CHECK (fulfillment_status IN ('unfulfilled', 'partially_fulfilled', 'fulfilled'));

-- Set together, or not at all.
ALTER TABLE orders
    ADD COLUMN cancelled_at  timestamptz,
    ADD COLUMN cancel_reason text
        CHECK (cancel_reason IN ('customer', 'inventory', 'fraud', 'declined', 'staff', 'other')),
    ADD CHECK (num_nulls(cancelled_at, cancel_reason) IN (0, 2));

-- A removed variant's item with units still reserved: deleted once they're gone.
ALTER TABLE inventory_items ADD COLUMN retired boolean NOT NULL DEFAULT false;
