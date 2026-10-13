-- The buyer on a cart (design note 0005, C5): both optional, so existing carts keep
-- working without a backfill. No foreign key to customers: linked by ID, checked by
-- set_cart_customer.

ALTER TABLE carts ADD COLUMN email text;
ALTER TABLE carts ADD COLUMN customer_id uuid;
