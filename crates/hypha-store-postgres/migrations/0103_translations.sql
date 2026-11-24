-- The store's content in its other languages. One row per resource and language: the
-- resource is named by its kind and UUID (no foreign key: one table for every kind; a
-- deleted resource's rows are removed by its event), `fields` holds the translated
-- fields by key, each with the digest of the source text it translated:
-- {"title": {"value": "Tasse", "source_digest": "<sha-256 hex>"}}.
--
-- Kind and language compare as bytes, so the key's order is the same everywhere a list
-- is continued from it.
CREATE TABLE translations (
    resource_kind text        COLLATE "C" NOT NULL,
    resource_id   uuid        NOT NULL,
    language      text        COLLATE "C" NOT NULL,
    version       bigint      NOT NULL CHECK (version > 0),
    fields        jsonb       NOT NULL,
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT translations_pkey PRIMARY KEY (resource_kind, resource_id, language)
);

-- A language's translations: the list filtered by language, and removing a language.
CREATE INDEX translations_language ON translations (language, resource_kind, resource_id);

CREATE TRIGGER hypha_note_write AFTER INSERT OR UPDATE OR DELETE ON translations
    FOR EACH ROW WHEN (current_setting('hypha.log_writes', true) = 'on')
    EXECUTE FUNCTION hypha_note_write('resource_kind', 'resource_id', 'language');
