-- Locations, inventory items, stock levels and reservations (the hypha-inventory crate,
-- design note 0004). Constraint names are part of the adapter contract: error.rs maps
-- them to `hypha_inventory::repo::keys`.

CREATE TABLE locations (
    id       uuid   CONSTRAINT locations_pkey PRIMARY KEY,
    version  bigint NOT NULL CHECK (version > 0),
    name     text   NOT NULL,
    priority bigint NOT NULL CHECK (priority BETWEEN 0 AND 4294967295)
);

-- No foreign key to markets: other domains are linked by ID only (design note 0002).
CREATE TABLE location_markets (
    location_id uuid    NOT NULL REFERENCES locations (id) ON DELETE CASCADE,
    position    integer NOT NULL,
    market_id   uuid    NOT NULL,
    PRIMARY KEY (location_id, market_id),
    UNIQUE (location_id, position)
);

CREATE INDEX location_markets_market ON location_markets (market_id);

-- One item per variant; no foreign key to variants (linked by ID only).
CREATE TABLE inventory_items (
    id               uuid    CONSTRAINT inventory_items_pkey PRIMARY KEY,
    version          bigint  NOT NULL CHECK (version > 0),
    variant_id       uuid    NOT NULL CONSTRAINT inventory_item_variant UNIQUE,
    tracked          boolean NOT NULL,
    continue_selling boolean NOT NULL
);

-- Counts stay within ±(2^53 - 1), `MAX_STOCK`. No foreign key to locations: a separate
-- aggregate, checked by the commands that write stock.
CREATE TABLE inventory_levels (
    item_id     uuid    NOT NULL REFERENCES inventory_items (id) ON DELETE CASCADE,
    position    integer NOT NULL,
    location_id uuid    NOT NULL,
    on_hand     bigint  NOT NULL CHECK (on_hand BETWEEN -9007199254740991 AND 9007199254740991),
    PRIMARY KEY (item_id, location_id),
    UNIQUE (item_id, position)
);

CREATE TABLE inventory_reservations (
    item_id     uuid    NOT NULL REFERENCES inventory_items (id) ON DELETE CASCADE,
    position    integer NOT NULL,
    reference   text    NOT NULL,
    location_id uuid    NOT NULL,
    quantity    bigint  NOT NULL CHECK (quantity BETWEEN 1 AND 4294967295),
    PRIMARY KEY (item_id, reference, location_id),
    UNIQUE (item_id, position)
);
