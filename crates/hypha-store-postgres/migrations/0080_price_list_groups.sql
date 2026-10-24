-- A price list's audience: the customer groups it prices for (none: everyone). A group an
-- audience names can't be deleted, and an audience names only groups that exist; both are
-- domain rules (`RepoError::Reference`).
CREATE TABLE price_list_groups (
    price_list_id uuid NOT NULL REFERENCES price_lists (id) ON DELETE CASCADE,
    group_id      uuid NOT NULL CONSTRAINT price_list_group REFERENCES customer_groups (id),
    PRIMARY KEY (price_list_id, group_id)
);
CREATE INDEX price_list_groups_group ON price_list_groups (group_id);
