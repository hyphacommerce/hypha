-- A checked-out cart records its order (design note 0007, O6). Null means the cart is
-- still open, so existing carts need no backfill. No foreign key: orders are a
-- separate aggregate, written in the same transaction by checkout.
ALTER TABLE carts ADD COLUMN order_id uuid;
