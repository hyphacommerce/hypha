-- Markets, price lists and catalogs (the hypha-markets crate). Constraint names are part
-- of the adapter contract: error.rs maps them to `hypha_markets::repo::keys`.

CREATE TABLE markets (
    id       uuid   CONSTRAINT markets_pkey PRIMARY KEY,
    version  bigint NOT NULL CHECK (version > 0),
    name     text   NOT NULL,
    currency text   NOT NULL
);

-- A country belongs to at most one market: the country is the primary key.
CREATE TABLE market_countries (
    country   text    CONSTRAINT market_country PRIMARY KEY,
    market_id uuid    NOT NULL REFERENCES markets (id) ON DELETE CASCADE,
    position  integer NOT NULL,
    UNIQUE (market_id, position)
);

CREATE TABLE price_lists (
    id         uuid    CONSTRAINT price_lists_pkey PRIMARY KEY,
    version    bigint  NOT NULL CHECK (version > 0),
    currency   text    NOT NULL,
    adjustment integer NOT NULL CHECK (adjustment >= -10000)
);

-- Fixed prices live in their own table and are read and written in batches.
-- No foreign key to variants: they belong to another aggregate (and crate).
CREATE TABLE price_list_prices (
    price_list_id uuid   NOT NULL REFERENCES price_lists (id) ON DELETE CASCADE,
    variant_id    uuid   NOT NULL,
    amount        bigint NOT NULL CHECK (amount >= 0),
    currency      text   NOT NULL,
    PRIMARY KEY (price_list_id, variant_id)
);

-- No foreign keys to markets or price lists: separate aggregates, checked by commands.
CREATE TABLE catalogs (
    id            uuid   CONSTRAINT catalogs_pkey PRIMARY KEY,
    version       bigint NOT NULL CHECK (version > 0),
    context_kind  text   NOT NULL CHECK (context_kind IN ('market')),
    context_id    uuid   NOT NULL,
    price_list_id uuid,
    CONSTRAINT catalog_context UNIQUE (context_kind, context_id)
);

CREATE TABLE catalog_products (
    catalog_id uuid NOT NULL REFERENCES catalogs (id) ON DELETE CASCADE,
    product_id uuid NOT NULL,
    PRIMARY KEY (catalog_id, product_id)
);
