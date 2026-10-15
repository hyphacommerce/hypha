-- Where assets are used (design notes 0013, section 6, and 0014, section 6): written in
-- the transaction that writes their owner, and replaced wholesale per owner.
CREATE TABLE asset_usages (
    -- The constraint's name is the repo key `asset_usage_asset`: a usage of a missing
    -- asset, or deleting an asset that has usages, fails with it. Both tables are
    -- media's, so the reference is the database's to keep (0014, E10).
    -- NO ACTION rather than RESTRICT: both refuse, but only NO ACTION reports a foreign
    -- key violation (23503), the code the adapter maps.
    asset_id   uuid NOT NULL
               CONSTRAINT asset_usage_asset REFERENCES assets (id) ON DELETE NO ACTION,
    -- Byte order, so pages of owners sort the same here and in memory.
    owner_kind text COLLATE "C" NOT NULL CHECK (owner_kind ~ '^[a-z][a-z0-9_]*$'),
    owner_id   text COLLATE "C" NOT NULL,
    role       text NOT NULL CHECK (role ~ '^[a-z][a-z0-9_]*$'),
    PRIMARY KEY (owner_kind, owner_id, asset_id)
);

-- "Used by": one asset's usages by owner, last first.
CREATE INDEX asset_usages_by_asset ON asset_usages (asset_id, owner_kind, owner_id);
