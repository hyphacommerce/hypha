-- The store a deployment serves (design note 0006). With one store per deployment
-- (model A) this table has exactly one row; other tables don't reference it, because
-- tenancy lives in the ports, not in the rows.

CREATE TABLE stores (
    id      uuid   CONSTRAINT stores_pkey PRIMARY KEY,
    version bigint NOT NULL CHECK (version > 0),
    name    text   NOT NULL CHECK (length(btrim(name)) > 0)
);
