-- How far the schedule job has announced price lists' window boundaries
-- (`price_list.started` / `.ended`): one row, written with the events it announced. No row
-- until the job's first run, which only notes where it starts.
CREATE TABLE price_list_schedule (
    id        boolean     PRIMARY KEY DEFAULT true CHECK (id),
    ran_until timestamptz NOT NULL
);

-- Search documents carry the compare-at price a shopper sees struck through beside the
-- price (in the same currency; NULL for none), and `on_sale` among their facets.
-- Documents written before have neither: drop them, with the option counts kept
-- alongside. The host rebuilds an empty index when it starts.
TRUNCATE search_documents, search_option_counts, search_channel_option_counts;

ALTER TABLE search_documents ADD COLUMN compare_at bigint;
