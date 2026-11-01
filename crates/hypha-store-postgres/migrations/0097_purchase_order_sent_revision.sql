-- The revision a purchase order was last sent at: once it's edited past it, the supplier
-- holds an older copy. Orders sent before this were sent at their revision then, which is
-- the best guess for their current one.
ALTER TABLE purchase_orders ADD COLUMN sent_revision integer
    CHECK (sent_revision IS NULL OR sent_revision >= 0);

UPDATE purchase_orders SET sent_revision = revision WHERE sent_at IS NOT NULL;
