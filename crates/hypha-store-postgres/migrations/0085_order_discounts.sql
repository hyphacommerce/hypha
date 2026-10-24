-- Discounts on orders: what applied (copies, one per reward, as JSON), and each line's
-- and shipping line's shares. A line's `total` is now after its discounts; `subtotal` is
-- its unit price times its quantity, which is what `total` was for every order placed
-- before (none had discounts), so they keep reading the same.
ALTER TABLE orders
    ADD COLUMN discounts      jsonb  NOT NULL DEFAULT '[]',
    ADD COLUMN discount_total bigint NOT NULL DEFAULT 0;

ALTER TABLE order_lines
    ADD COLUMN subtotal  bigint,
    ADD COLUMN discounts jsonb NOT NULL DEFAULT '[]';
UPDATE order_lines SET subtotal = total;
ALTER TABLE order_lines ALTER COLUMN subtotal SET NOT NULL;

ALTER TABLE order_shipping_lines
    ADD COLUMN total     bigint,
    ADD COLUMN discounts jsonb NOT NULL DEFAULT '[]';
UPDATE order_shipping_lines SET total = amount;
ALTER TABLE order_shipping_lines ALTER COLUMN total SET NOT NULL;
