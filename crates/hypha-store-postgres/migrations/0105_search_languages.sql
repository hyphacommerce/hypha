-- Search in every language a market offers: one document per variant, market and
-- language. Text a shopper reads is in the document's language; facet values stay as
-- stored, each with its label in that language beside it.

ALTER TABLE search_documents
    ADD COLUMN language text,
    -- The options as the language reads them, aligned with `option_names` and
    -- `option_values` (which stay as stored: facet keys and values).
    ADD COLUMN option_name_labels text[],
    ADD COLUMN option_value_labels text[],
    -- One label per entry of `facets`, in the same order.
    ADD COLUMN facet_labels text[];

-- The documents written before were in the store's own words: they become the
-- documents of each market's default language (its first listed language the store
-- offers: the default or a published one; else the store's default), labelled as
-- stored. A market whose default isn't the store's then shows them untranslated until
-- the rebuild queued below replaces them, with its other languages.
UPDATE search_documents d
SET language = coalesce(
        (SELECT l
         FROM markets m, unnest(m.languages) WITH ORDINALITY AS listed (l, n), stores s
         WHERE m.id = d.market_id
           AND (l = s.default_language
                OR s.languages @> jsonb_build_array(
                       jsonb_build_object('language', l, 'published', true)))
         ORDER BY n LIMIT 1),
        (SELECT default_language FROM stores LIMIT 1),
        'en'),
    option_name_labels = option_names,
    option_value_labels = option_values,
    facet_labels = coalesce(
        (SELECT array_agg(substr(fv, strpos(fv, chr(31)) + 1) ORDER BY n)
         FROM unnest(d.facets) WITH ORDINALITY AS f (fv, n)),
        '{}');

ALTER TABLE search_documents
    ALTER COLUMN language SET NOT NULL,
    ALTER COLUMN option_name_labels SET NOT NULL,
    ALTER COLUMN option_value_labels SET NOT NULL,
    ALTER COLUMN facet_labels SET NOT NULL,
    DROP CONSTRAINT search_documents_pkey,
    ADD PRIMARY KEY (market_id, language, variant_id);

-- Rebuild every product, in every language its markets offer.
INSERT INTO search_queue (target, generation)
SELECT 'all', 1 WHERE EXISTS (SELECT 1 FROM search_documents)
ON CONFLICT (target) DO UPDATE SET generation = search_queue.generation + 1;
