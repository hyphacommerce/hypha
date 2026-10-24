-- The admin's variant list across products: the newest product first, each product's
-- variants in their order, paged by (product, position). The unique (product_id, position)
-- index runs the other way for the product.
CREATE INDEX variants_by_product ON variants (product_id DESC, position);
