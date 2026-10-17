-- Categories (design note 0022): a tree by parent pointer, checked in memory by the
-- commands (`CategoryTree`), so there's no path column a move could leave wrong.
CREATE TABLE categories (
    id          uuid        PRIMARY KEY,
    version     bigint      NOT NULL CHECK (version > 0),
    name        text        NOT NULL CHECK (length(btrim(name)) > 0),
    -- The product handle rule; unique store-wide, so a category keeps its URL when it
    -- moves (CA6).
    handle      text        NOT NULL CHECK (handle ~ '^[a-z0-9]+(-[a-z0-9]+)*$'),
    -- Sanitised HTML (`Description`), at most 64 KiB.
    description text        CHECK (octet_length(description) <= 65536),
    -- An asset in use while set: its usage (owner `category`) is what keeps it.
    image_id    uuid,
    -- Checked at commit, so a batch can write a parent and its child in any order.
    parent_id   uuid        REFERENCES categories (id) DEFERRABLE INITIALLY DEFERRED,
    -- Dense among siblings (0 to n - 1), renumbered in the same batch as a move; not a
    -- unique key, which a renumbering would trip over halfway.
    position    integer     NOT NULL CHECK (position >= 0),
    visible     boolean     NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX category_handle ON categories (handle);
CREATE INDEX categories_parent ON categories (parent_id);

-- Membership. Product IDs aren't checked, like publications (a leaf can't see
-- products): an unknown one never matches. A category's membership goes with it.
CREATE TABLE category_products (
    category_id uuid NOT NULL REFERENCES categories (id) ON DELETE CASCADE,
    product_id  uuid NOT NULL,
    PRIMARY KEY (category_id, product_id)
);
CREATE INDEX category_products_product ON category_products (product_id);
