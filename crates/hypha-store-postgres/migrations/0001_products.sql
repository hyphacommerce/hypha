-- Products aggregate. Constraint names are part of the adapter contract: error.rs maps
-- them to the `keys::*` of each repo, so rename both together.

CREATE TABLE products (
    id      uuid   CONSTRAINT products_pkey PRIMARY KEY,
    version bigint NOT NULL CHECK (version > 0),
    handle  text   NOT NULL CONSTRAINT product_handle UNIQUE,
    title   text   NOT NULL,
    status  text   NOT NULL CHECK (status IN ('draft', 'active', 'archived'))
);

CREATE TABLE product_options (
    id         uuid    PRIMARY KEY,
    product_id uuid    NOT NULL REFERENCES products (id) ON DELETE CASCADE,
    position   integer NOT NULL,
    name       text    NOT NULL,
    UNIQUE (product_id, position)
);

CREATE TABLE product_option_values (
    id        uuid    PRIMARY KEY,
    option_id uuid    NOT NULL REFERENCES product_options (id) ON DELETE CASCADE,
    position  integer NOT NULL,
    label     text    NOT NULL,
    UNIQUE (option_id, position)
);

-- Variant IDs are unique across all products, hence the global primary key.
CREATE TABLE variants (
    id             uuid    CONSTRAINT variant_id PRIMARY KEY,
    product_id     uuid    NOT NULL REFERENCES products (id) ON DELETE CASCADE,
    position       integer NOT NULL,
    sku            text,
    price_amount   bigint  NOT NULL CHECK (price_amount >= 0),
    price_currency text    NOT NULL,
    -- One option value ID per option, in option order (empty for simple products).
    selections     uuid[]  NOT NULL,
    UNIQUE (product_id, position)
);

CREATE INDEX variants_product_id ON variants (product_id);
