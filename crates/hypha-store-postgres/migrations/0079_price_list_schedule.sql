-- How far the schedule job has announced price lists' window boundaries
-- (`price_list.started` / `.ended`): one row, written with the events it announced. No row
-- until the job's first run, which only notes where it starts.
CREATE TABLE price_list_schedule (
    id        boolean     PRIMARY KEY DEFAULT true CHECK (id),
    ran_until timestamptz NOT NULL
);

-- Search documents carry the compare-at price a shopper sees struck through beside the
-- price (in the same currency; NULL for none), and `on_sale` among their facets.
-- Documents written before have neither: they keep serving (as not on sale) while a full
-- rebuild, queued here as the search handler marks one, replaces them. An index with no
-- documents needs none (the host builds an empty index when it starts).
ALTER TABLE search_documents ADD COLUMN compare_at bigint;

INSERT INTO search_queue (target, generation)
SELECT 'all', 1 WHERE EXISTS (SELECT 1 FROM search_documents)
ON CONFLICT (target) DO UPDATE SET generation = search_queue.generation + 1;
