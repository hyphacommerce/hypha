-- Default facets per channel (design note 0020, review): a storefront's option facets
-- come from the documents its channel publishes, counted as documents are written like
-- the market's own (`search_option_counts`), so a search never scans to find them.

CREATE TABLE search_channel_option_counts (
    market_id  uuid   NOT NULL,
    channel_id uuid   NOT NULL,
    -- The facet key: `option.<name>`, the name lowercased.
    key        text   NOT NULL,
    documents  bigint NOT NULL,
    PRIMARY KEY (market_id, channel_id, key)
);

-- Documents written before have no channel counts: drop them. The host rebuilds an
-- empty index when it starts.
TRUNCATE search_documents, search_option_counts;
