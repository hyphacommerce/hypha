-- The provider's IDs of the refunds a payment counts (design note 0029, as built: review
-- fixes), oldest first, so a refund reported twice is counted once. Payments refunded
-- before this keep an empty list: their refunds were reported without one.
ALTER TABLE payments
    ADD COLUMN refund_references text[] NOT NULL DEFAULT '{}';
