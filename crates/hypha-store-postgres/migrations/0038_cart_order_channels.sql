-- Carts and orders record their sales channel (design note 0020, CH6 and CH7); it never
-- changes. Those made before channels belong to the Online Store (0036).
ALTER TABLE carts ADD COLUMN channel_id uuid;
ALTER TABLE orders ADD COLUMN channel_id uuid;
UPDATE carts SET channel_id = (SELECT id FROM channels ORDER BY created_at LIMIT 1);
UPDATE orders SET channel_id = (SELECT id FROM channels ORDER BY created_at LIMIT 1);
ALTER TABLE carts ALTER COLUMN channel_id SET NOT NULL;
ALTER TABLE orders ALTER COLUMN channel_id SET NOT NULL;
