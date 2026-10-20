-- Payment-first checkout (design note 0031): an online method's payment is started for
-- the cart before its order exists, and attached to the order checkout places once the
-- money is committed. `processing`: taken and being collected (SEPA: days).
ALTER TABLE payments
    ADD COLUMN cart_id uuid,
    ALTER COLUMN order_id DROP NOT NULL,
    DROP CONSTRAINT payments_status_check,
    ADD CONSTRAINT payments_status_check CHECK (status IN ('pending', 'authorized',
        'processing', 'paid', 'failed', 'voided', 'partially_refunded', 'refunded')),
    ADD CONSTRAINT payment_owner CHECK (cart_id IS NOT NULL OR order_id IS NOT NULL);

-- Payments checkout made so far were made for their order's cart.
UPDATE payments SET cart_id = orders.cart_id
FROM orders
WHERE orders.id = payments.order_id AND payments.method_id IS NOT NULL;

-- A cart's payments, newest first.
CREATE INDEX payments_by_cart ON payments (cart_id, id DESC) WHERE cart_id IS NOT NULL;

-- A `processing` payment is open too: its method can't be deleted.
DROP INDEX payments_open_by_method;
CREATE INDEX payments_open_by_method ON payments (method_id)
    WHERE status IN ('pending', 'authorized', 'processing');
