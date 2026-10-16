-- Search, faster (design note 0019, review): `tsv` is now built from the adapter's own
-- terms (`array_to_tsvector`), so it alone decides a text match; and the option names
-- each market's documents have are counted as documents are written, for the default
-- facets, instead of scanning the market on every search.

CREATE TABLE search_option_counts (
    market_id uuid   NOT NULL,
    -- The facet key: `option.<name>`, the name lowercased.
    key       text   NOT NULL,
    documents bigint NOT NULL,
    PRIMARY KEY (market_id, key)
);

-- Documents written before had `tsv` from Postgres's parser and no counts: drop them.
-- The host rebuilds an empty index when it starts.
TRUNCATE search_documents;
