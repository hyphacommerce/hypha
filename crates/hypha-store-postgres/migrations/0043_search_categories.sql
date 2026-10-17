-- Search by category (design note 0022, section 3): each document carries its
-- product's categories and all their ancestors, so a filter on "Clothing" finds what's
-- in "Shirts" too.

-- Documents written before carry no categories: drop them, with the option counts kept
-- alongside. The host rebuilds an empty index when it starts.
TRUNCATE search_documents, search_option_counts, search_channel_option_counts;

ALTER TABLE search_documents ADD COLUMN categories uuid[] NOT NULL;
CREATE INDEX search_documents_categories ON search_documents USING gin (categories);
