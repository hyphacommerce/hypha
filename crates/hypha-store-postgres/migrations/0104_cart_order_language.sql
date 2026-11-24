-- The language a shopper reads in (a BCP 47 tag) on carts, orders and customers, and the
-- names an order copied in it. NULL is the store's default: carts without a choice,
-- and every row from before languages, which keep reading as they did.
ALTER TABLE carts ADD COLUMN language text;
ALTER TABLE orders ADD COLUMN language text;
ALTER TABLE customers ADD COLUMN language text;

-- What the buyer read where it differs from the store's own text, kept beside it:
-- {"product_title": "Tasse", "variant_title": "Rot"} on a line, {"name": "Express"} on
-- a shipping line, {"method_name": …, "instructions": …} on a payment. A discount's
-- copy lives in `orders.discounts`, each item's `localized`. NULL where nothing differs.
ALTER TABLE order_lines ADD COLUMN localized jsonb;
ALTER TABLE order_shipping_lines ADD COLUMN localized jsonb;
ALTER TABLE payments ADD COLUMN localized jsonb;
