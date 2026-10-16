-- Sales channels (design note 0020): where products are sold, and which products each
-- channel sells.
CREATE TABLE channels (
    id         uuid        PRIMARY KEY,
    version    bigint      NOT NULL,
    name       text        NOT NULL,
    -- The name as uniqueness compares it (`Channel::name_key`): lowercased by the
    -- domain, not by lower(), whose idea of case depends on the database's collation.
    name_key   text        NOT NULL,
    kind       text        NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);
-- Names are unique ignoring case.
CREATE UNIQUE INDEX channel_name ON channels (name_key);

-- Publications. Product IDs aren't checked, like catalog membership (a leaf can't see
-- products): an unknown one never matches.
CREATE TABLE channel_products (
    channel_id uuid NOT NULL REFERENCES channels (id) ON DELETE CASCADE,
    product_id uuid NOT NULL,
    PRIMARY KEY (channel_id, product_id)
);
CREATE INDEX channel_products_product ON channel_products (product_id);

-- Every store starts with an Online Store channel selling what it sold before (CH9).
INSERT INTO channels (id, version, name, name_key, kind)
VALUES (gen_random_uuid(), 1, 'Online Store', 'online store', 'online_store');
INSERT INTO channel_products (channel_id, product_id)
SELECT c.id, p.id FROM channels c CROSS JOIN products p;
