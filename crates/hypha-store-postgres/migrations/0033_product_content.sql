-- What a product says about itself (design note 0019, section 2). The description is
-- sanitised HTML; tags keep the merchant's order.
ALTER TABLE products
    ADD COLUMN description  text,
    ADD COLUMN vendor       text,
    ADD COLUMN product_type text,
    ADD COLUMN tags         text[] NOT NULL DEFAULT '{}';
