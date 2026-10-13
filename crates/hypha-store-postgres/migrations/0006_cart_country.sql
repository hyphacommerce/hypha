-- A cart ships to a country, which picks its market and its tax rates (design note 0003,
-- T2 and T9). There are no real carts yet (T10), so existing ones are deleted instead of
-- backfilled; their lines go with them (ON DELETE CASCADE).

DELETE FROM carts;
ALTER TABLE carts ADD COLUMN country text NOT NULL;
