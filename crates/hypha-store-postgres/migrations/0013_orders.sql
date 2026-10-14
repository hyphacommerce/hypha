-- Orders (the hypha-orders crate, design note 0007). Constraint names are part of the
-- adapter contract: error.rs maps them to `hypha_orders::repo::keys`.

-- Order numbers: `nextval` never rolls back, so a failed checkout leaves a gap, and no
-- checkout waits for another to get its number.
CREATE SEQUENCE order_numbers START 1001;

-- No foreign keys to carts, markets or customers: separate aggregates. Every amount is
-- in minor units of `currency` (the domain checks it when loading).
CREATE TABLE orders (
    id                 uuid        CONSTRAINT orders_pkey PRIMARY KEY,
    version            bigint      NOT NULL CHECK (version > 0),
    number             bigint      NOT NULL CONSTRAINT order_number UNIQUE CHECK (number > 0),
    placed_at          timestamptz NOT NULL,
    cart_id            uuid        NOT NULL CONSTRAINT order_cart UNIQUE,
    market_id          uuid        NOT NULL,
    currency           text        NOT NULL,
    country            text        NOT NULL,
    taxes_included     boolean     NOT NULL,
    customer_id        uuid        NOT NULL,
    email              text        NOT NULL,
    subtotal           bigint      NOT NULL,
    tax_total          bigint      NOT NULL,
    total              bigint      NOT NULL,
    financial_status   text        NOT NULL CHECK (financial_status IN ('pending', 'paid')),
    fulfillment_status text        NOT NULL CHECK (fulfillment_status IN ('unfulfilled'))
);

-- Lines are copies (titles, prices, tax), so no foreign keys to products either. The
-- tax columns are all set or all null.
CREATE TABLE order_lines (
    id             uuid    CONSTRAINT order_lines_pkey PRIMARY KEY,
    order_id       uuid    NOT NULL REFERENCES orders (id) ON DELETE CASCADE,
    position       integer NOT NULL,
    product_id     uuid    NOT NULL,
    variant_id     uuid    NOT NULL,
    quantity       bigint  NOT NULL CHECK (quantity BETWEEN 1 AND 4294967295),
    product_title  text    NOT NULL,
    variant_title  text,
    sku            text,
    unit_price     bigint  NOT NULL,
    total          bigint  NOT NULL,
    tax_rate_ppm   integer,
    tax_class_id   uuid,
    tax_amount     bigint,
    tax_region_id  uuid,
    CHECK (num_nulls(tax_rate_ppm, tax_amount, tax_region_id) IN (0, 3)),
    UNIQUE (order_id, position)
);
