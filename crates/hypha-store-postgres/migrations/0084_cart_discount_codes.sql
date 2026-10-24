-- The discount codes a cart holds, normalised, in the order entered (at most 5).
ALTER TABLE carts ADD COLUMN discount_codes text[] NOT NULL DEFAULT '{}';
