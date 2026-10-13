-- Customers and their address books (the hypha-customers crate, design note 0005).
-- Constraint names are part of the adapter contract: error.rs maps them to
-- `hypha_customers::repo::keys`.

-- Emails are stored lowercased by the domain, so a plain unique key is case-insensitive.
-- The defaults point into customer_addresses; the domain checks that when loading.
CREATE TABLE customers (
    id                          uuid   CONSTRAINT customers_pkey PRIMARY KEY,
    version                     bigint NOT NULL CHECK (version > 0),
    email                       text   NOT NULL CONSTRAINT customer_email UNIQUE,
    first_name                  text,
    last_name                   text,
    phone                       text,
    default_shipping_address_id uuid,
    default_billing_address_id  uuid
);

CREATE TABLE customer_addresses (
    customer_id uuid    NOT NULL REFERENCES customers (id) ON DELETE CASCADE,
    position    integer NOT NULL,
    id          uuid    NOT NULL,
    first_name  text,
    last_name   text    NOT NULL,
    company     text,
    line1       text    NOT NULL,
    line2       text,
    city        text    NOT NULL,
    province    text,
    postal_code text,
    country     text    NOT NULL,
    phone       text,
    PRIMARY KEY (customer_id, id),
    UNIQUE (customer_id, position)
);
