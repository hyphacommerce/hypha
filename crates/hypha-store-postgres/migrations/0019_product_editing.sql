-- Product editing and product media (design note 0014).

-- Rows written before the editing rules (E7) must still load, since `restore` re-runs
-- them. SKUs are trimmed and blank ones cleared, as the domain does; anything the domain
-- would refuse fails the migration, naming the products to fix by hand.
UPDATE variants SET sku = NULLIF(btrim(sku, E' \t\r\n'), '') WHERE sku IS NOT NULL;

DO $$
DECLARE
    bad text;
BEGIN
    SELECT string_agg(DISTINCT product_id::text, ', ') INTO bad FROM (
        -- Option names and value labels: required, at most 255 characters.
        SELECT product_id FROM product_options
        WHERE btrim(name, E' \t\r\n') = '' OR char_length(btrim(name, E' \t\r\n')) > 255
        UNION ALL
        SELECT o.product_id FROM product_option_values v
        JOIN product_options o ON o.id = v.option_id
        WHERE btrim(v.label, E' \t\r\n') = '' OR char_length(btrim(v.label, E' \t\r\n')) > 255
        -- Option names unique within a product, labels within an option, ignoring case.
        UNION ALL
        SELECT product_id FROM product_options
        GROUP BY product_id, lower(name) HAVING count(*) > 1
        UNION ALL
        SELECT o.product_id FROM product_option_values v
        JOIN product_options o ON o.id = v.option_id
        GROUP BY o.product_id, v.option_id, lower(v.label) HAVING count(*) > 1
        -- SKUs: at most 255 characters, and unique (Q4).
        UNION ALL
        SELECT product_id FROM variants WHERE char_length(sku) > 255
        UNION ALL
        SELECT product_id FROM variants
        WHERE sku IN (SELECT sku FROM variants GROUP BY sku HAVING count(*) > 1)
    ) failing;
    IF bad IS NOT NULL THEN
        RAISE EXCEPTION 'products % break the editing rules of design note 0014 (option names, labels or SKUs); fix them, then migrate again', bad;
    END IF;
END $$;

-- A set SKU is unique across all variants. The name is the repo key `variant_sku`.
CREATE UNIQUE INDEX variant_sku ON variants (sku) WHERE sku IS NOT NULL;

-- The ordered media list; position 0 is the featured item. No foreign key to assets:
-- media is another module (design note 0002, section 7); asset_usages guards deletes.
CREATE TABLE product_media (
    product_id uuid    NOT NULL REFERENCES products (id) ON DELETE CASCADE,
    position   integer NOT NULL CHECK (position >= 0),
    asset_id   uuid    NOT NULL,
    PRIMARY KEY (product_id, position),
    UNIQUE (product_id, asset_id)
);

-- One item of the product's media list, or none.
ALTER TABLE variants ADD COLUMN media_asset_id uuid;
