-- A catalog holds any number of price lists, which compete to price its products (the
-- lowest live price wins). Each list a catalog had moves into the set; the column goes.
-- A list on a catalog can't be deleted: the reference is a domain rule
-- (`RepoError::Reference`).
CREATE TABLE catalog_price_lists (
    catalog_id    uuid NOT NULL REFERENCES catalogs (id) ON DELETE CASCADE,
    price_list_id uuid NOT NULL CONSTRAINT catalog_price_list REFERENCES price_lists (id),
    PRIMARY KEY (catalog_id, price_list_id)
);
CREATE INDEX catalog_price_lists_list ON catalog_price_lists (price_list_id);

INSERT INTO catalog_price_lists (catalog_id, price_list_id)
SELECT c.id, c.price_list_id FROM catalogs c JOIN price_lists p ON p.id = c.price_list_id;

ALTER TABLE catalogs DROP COLUMN price_list_id;

-- What an adjustment reaches: every product (as every existing list did), listed products
-- or listed categories (with the categories below them). The IDs are other domains', not
-- checked: one that names nothing reaches nothing.
ALTER TABLE price_lists
    ADD COLUMN applies_to text NOT NULL DEFAULT 'all'
        CONSTRAINT price_list_applies_to CHECK (applies_to IN ('all', 'products', 'categories')),
    ADD COLUMN applies_to_ids uuid[] NOT NULL DEFAULT '{}'
        CONSTRAINT price_list_applies_to_ids CHECK (cardinality(applies_to_ids) <= 1000);

-- The list's sorts: by name (as `SortKey::text` compares) and by ID.
CREATE INDEX price_lists_name_order ON price_lists ((lower(name) COLLATE "C"), id);
