-- Custom fields (design note 0033): list `contains` filters compare items as
-- `hypha_custom_fields::list_item` does (strings lowercased, as `scalar_text` lowercases
-- single values), against a column the domain derives (`ValueRow::list_items`); and the
-- unique key on values is checked when a transaction commits, so one batch may move a
-- value from one owner to another (or swap two) whatever order its writes run in.
ALTER TABLE custom_field_values ADD COLUMN list_items jsonb;

-- Lists stored before: Postgres' `lower` stands in for Rust's `to_lowercase` (the same
-- for every letter but a few special cases, which the next write of the row corrects).
UPDATE custom_field_values
SET list_items = (
    SELECT coalesce(jsonb_agg(CASE WHEN jsonb_typeof(i.item) = 'string'
                                   THEN to_jsonb(lower(i.item #>> '{}'))
                                   ELSE i.item END
                              ORDER BY i.n), '[]'::jsonb)
    FROM jsonb_array_elements(value) WITH ORDINALITY AS i (item, n))
WHERE jsonb_typeof(value) = 'array';

CREATE INDEX custom_field_values_list_items
    ON custom_field_values USING gin (list_items jsonb_path_ops)
    WHERE list_items IS NOT NULL;

ALTER TABLE custom_field_values
    DROP CONSTRAINT custom_field_unique_value,
    ADD CONSTRAINT custom_field_unique_value UNIQUE (definition_id, unique_text)
        DEFERRABLE INITIALLY DEFERRED;
