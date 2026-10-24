-- Price lists get a name, a description, a kind, a status, a window and a compare-at
-- mode, and their adjustment becomes optional: a list without one prices only its fixed
-- variants. Existing lists keep pricing as they did: active `override` lists without a
-- window, their adjustment kept (0 included).
ALTER TABLE price_lists
    ADD COLUMN name        text,
    ADD COLUMN description text,
    ADD COLUMN kind        text NOT NULL DEFAULT 'override'
        CONSTRAINT price_list_kind CHECK (kind IN ('sale', 'override')),
    ADD COLUMN status      text NOT NULL DEFAULT 'active'
        CONSTRAINT price_list_status CHECK (status IN ('draft', 'active')),
    ADD COLUMN starts_at   timestamptz,
    ADD COLUMN ends_at     timestamptz,
    ADD COLUMN compare_at  text NOT NULL DEFAULT 'adjusted'
        CONSTRAINT price_list_compare_at CHECK (compare_at IN ('adjusted', 'hidden')),
    ALTER COLUMN adjustment DROP NOT NULL,
    ADD CONSTRAINT price_list_window CHECK (ends_at > starts_at);

-- A list a catalog uses is named after its first market ("Eurozone prices"); the others
-- are numbered by age. Names are unique ignoring case, so a repeated one gets " (2)",
-- " (3)", … in the order the lists were made.
WITH named AS (
    SELECT pl.id,
           COALESCE(
               (SELECT left(m.name, 240) || ' prices'
                FROM catalogs c JOIN markets m ON m.id = c.context_id
                WHERE c.context_kind = 'market' AND c.price_list_id = pl.id
                ORDER BY m.id LIMIT 1),
               'Price list ' || row_number() OVER (ORDER BY pl.id)
           ) AS base
    FROM price_lists pl
), numbered AS (
    SELECT id, base, row_number() OVER (PARTITION BY lower(base) ORDER BY id) AS n
    FROM named
)
UPDATE price_lists p
SET name = CASE WHEN numbered.n = 1 THEN numbered.base
                ELSE numbered.base || ' (' || numbered.n || ')' END
FROM numbered
WHERE p.id = numbered.id;

ALTER TABLE price_lists ALTER COLUMN name SET NOT NULL;
CREATE UNIQUE INDEX price_list_name ON price_lists (lower(name));
