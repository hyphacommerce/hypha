-- Carts (the hypha-cart crate). Constraint names are part of the adapter contract:
-- error.rs maps them to `hypha_cart::repo::keys`.

-- No foreign key to markets: a separate aggregate, checked by create_cart.
CREATE TABLE carts (
    id        uuid   CONSTRAINT carts_pkey PRIMARY KEY,
    version   bigint NOT NULL CHECK (version > 0),
    market_id uuid   NOT NULL
);

-- No foreign keys to products or variants either: a line can outlive its product
-- (the cart then reports it as unavailable).
CREATE TABLE cart_lines (
    cart_id    uuid    NOT NULL REFERENCES carts (id) ON DELETE CASCADE,
    position   integer NOT NULL,
    product_id uuid    NOT NULL,
    variant_id uuid    NOT NULL,
    -- Quantity is a u32 in the domain: 1 ..= 4294967295.
    quantity   bigint  NOT NULL CHECK (quantity BETWEEN 1 AND 4294967295),
    PRIMARY KEY (cart_id, variant_id),
    UNIQUE (cart_id, position)
);
