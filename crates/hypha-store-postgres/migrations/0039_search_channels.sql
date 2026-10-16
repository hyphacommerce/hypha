-- Search by channel (design note 0020, section 5): each document carries the channels
-- its product is published to, and a storefront's search keeps to its own, default
-- facets included.

-- Documents written before carry no channels, so a storefront would find nothing: drop
-- them, with the option counts kept alongside. The host rebuilds an empty index when it
-- starts.
TRUNCATE search_documents, search_option_counts;

ALTER TABLE search_documents ADD COLUMN channels uuid[] NOT NULL;
CREATE INDEX search_documents_channels ON search_documents USING gin (channels);

-- A storefront's default option facets come from the documents its channel publishes,
-- counted as documents are written like the market's own (`search_option_counts`), so
-- a search never scans to find them.
CREATE TABLE search_channel_option_counts (
    market_id  uuid   NOT NULL,
    channel_id uuid   NOT NULL,
    -- The facet key: `option.<name>`, the name lowercased.
    key        text   NOT NULL,
    documents  bigint NOT NULL,
    PRIMARY KEY (market_id, channel_id, key)
);
