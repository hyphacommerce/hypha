-- What an order charged for shipping, copied at checkout (design note 0015, SH12).
ALTER TABLE orders
    ADD COLUMN shipping_first_name  text,
    ADD COLUMN shipping_last_name   text,
    ADD COLUMN shipping_company     text,
    ADD COLUMN shipping_line1       text,
    ADD COLUMN shipping_line2       text,
    ADD COLUMN shipping_city        text,
    ADD COLUMN shipping_province    text,
    ADD COLUMN shipping_postal_code text,
    ADD COLUMN shipping_country     text,
    ADD COLUMN shipping_phone       text,
    -- Orders placed before shipping charged none.
    ADD COLUMN shipping_total       bigint NOT NULL DEFAULT 0
        CHECK (shipping_total BETWEEN 0 AND 9007199254740991),
    ADD CONSTRAINT order_shipping_address_whole CHECK (
        (shipping_last_name IS NULL) = (shipping_line1 IS NULL)
        AND (shipping_line1 IS NULL) = (shipping_city IS NULL)
        AND (shipping_city IS NULL) = (shipping_country IS NULL)
    );

-- Copies, so no foreign keys to options or providers. The tax columns are all set or
-- all null, like order_lines'.
CREATE TABLE order_shipping_lines (
    order_id          uuid    NOT NULL REFERENCES orders (id) ON DELETE CASCADE,
    position          integer NOT NULL,
    group_id          uuid    NOT NULL,
    -- The order lines it ships (order_lines.id).
    line_ids          uuid[]  NOT NULL,
    option_id         uuid    NOT NULL,
    code              text    NOT NULL,
    name              text    NOT NULL,
    provider          text,
    service           text,
    reference         text,
    amount            bigint  NOT NULL CHECK (amount BETWEEN 0 AND 9007199254740991),
    estimate_min_days integer CHECK (estimate_min_days BETWEEN 0 AND 65535),
    estimate_max_days integer CHECK (estimate_max_days BETWEEN 0 AND 65535),
    tax_rate_ppm      integer,
    tax_class_id      uuid,
    tax_amount        bigint,
    tax_region_id     uuid,
    CHECK (num_nulls(tax_rate_ppm, tax_amount, tax_region_id) IN (0, 3)),
    CHECK ((estimate_min_days IS NULL) = (estimate_max_days IS NULL)),
    PRIMARY KEY (order_id, position)
);
