-- Customer groups and their membership (design note 0032, §6): named, static sets of
-- customers with a stable ID other domains point at. Constraint names are part of the
-- adapter contract: error.rs maps them to `hypha_customers::repo::keys`.
CREATE TABLE customer_groups (
    id          uuid   CONSTRAINT customer_groups_pkey PRIMARY KEY,
    version     bigint NOT NULL CHECK (version > 0),
    name        text   NOT NULL CHECK (length(btrim(name)) > 0),
    handle      text   NOT NULL CONSTRAINT customer_group_handle UNIQUE
                       CHECK (handle ~ '^[a-z0-9]+(-[a-z0-9]+)*$'),
    description text,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now()
);
-- Names are unique ignoring case.
CREATE UNIQUE INDEX customer_group_name ON customer_groups (lower(name));

CREATE TABLE customer_group_members (
    group_id    uuid NOT NULL REFERENCES customer_groups (id) ON DELETE CASCADE,
    customer_id uuid NOT NULL REFERENCES customers (id) ON DELETE CASCADE,
    PRIMARY KEY (group_id, customer_id)
);
CREATE INDEX customer_group_members_customer ON customer_group_members (customer_id);
