-- Channel names compared one way everywhere (design note 0020, review): the unique
-- index was on lower(name), whose idea of case depends on the database's collation
-- (under C, lower('É') is 'É'), while the domain lowercases by Rust's Unicode rules.
-- Adapters now store the domain's key (`Channel::name_key`) and index that.

ALTER TABLE channels ADD COLUMN name_key text;
-- Rows so far (the Online Store, and channels from before this migration) get
-- Postgres's lowercasing, which agrees with Rust's for ASCII names; the next rename
-- rewrites the key the domain's way.
UPDATE channels SET name_key = lower(btrim(name));
ALTER TABLE channels ALTER COLUMN name_key SET NOT NULL;

DROP INDEX channel_name;
CREATE UNIQUE INDEX channel_name ON channels (name_key);
