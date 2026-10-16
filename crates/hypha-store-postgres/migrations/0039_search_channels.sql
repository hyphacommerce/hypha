-- Search by channel (design note 0020, section 5): each document carries the channels
-- its product is published to, and a storefront's search keeps to its own.

-- Documents written before carry no channels, so a storefront would find nothing: drop
-- them, with the option counts kept alongside. The host rebuilds an empty index when it
-- starts.
TRUNCATE search_documents, search_option_counts;

ALTER TABLE search_documents ADD COLUMN channels uuid[] NOT NULL;
CREATE INDEX search_documents_channels ON search_documents USING gin (channels);
