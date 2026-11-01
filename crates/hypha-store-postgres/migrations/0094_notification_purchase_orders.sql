-- Emails to suppliers name the purchase order they're about, as order emails name their
-- order. No key: a message's record outlives what it was about.
ALTER TABLE notifications ADD COLUMN purchase_order_id uuid;

CREATE INDEX notifications_by_purchase_order ON notifications (purchase_order_id, id DESC)
    WHERE purchase_order_id IS NOT NULL;
