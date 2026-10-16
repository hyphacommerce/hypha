-- Search (design note 0019): the built-in engine's documents, the queue of what to
-- rebuild, and the merchant's settings.

-- One row per variant per market, as `hypha_search::SearchDocument`. The terms are
-- written by the adapter from `SearchDocument::terms` (one tokenizer for documents and
-- queries), and the score and facets are computed from them exactly as
-- `hypha_search::reference` does; `tsv` only narrows text searches through its index.
CREATE TABLE search_documents (
    market_id      uuid    NOT NULL,
    product_id     uuid    NOT NULL,
    variant_id     uuid    NOT NULL,
    listing        text    NOT NULL,
    handle         text    NOT NULL,
    title          text    NOT NULL,
    -- `title` lowercased in Rust, so "title" order is the reference's (byte order).
    title_sort     text    NOT NULL,
    variant_title  text,
    description    text    NOT NULL,
    vendor         text,
    product_type   text,
    tags           text[]  NOT NULL,
    sku            text,
    option_names   text[]  NOT NULL,
    option_values  text[]  NOT NULL,
    currency       text    NOT NULL,
    price          bigint  NOT NULL,
    product_min    bigint  NOT NULL,
    product_max    bigint  NOT NULL,
    listing_min    bigint  NOT NULL,
    listing_max    bigint  NOT NULL,
    available      boolean NOT NULL,
    image_id       uuid,
    -- The product's creation, in microseconds since the epoch.
    created_us     bigint  NOT NULL,
    position       integer NOT NULL,
    terms_a        text[]  NOT NULL,
    terms_b        text[]  NOT NULL,
    terms_d        text[]  NOT NULL,
    tsv            tsvector NOT NULL,
    -- `key || chr(31) || value` per facet value (`option.color` 0x1F `Red`).
    facets         text[]  NOT NULL,
    PRIMARY KEY (market_id, variant_id)
);
CREATE INDEX search_documents_product ON search_documents (market_id, product_id);
CREATE INDEX search_documents_tsv ON search_documents USING gin (tsv);
CREATE INDEX search_documents_facets ON search_documents USING gin (facets);

-- One row per target however often it's marked: `product:<uuid>`, `variant:<uuid>`,
-- `all`. `seq` orders leases (oldest marked first); `generation` goes up on every mark.
CREATE TABLE search_queue (
    target       text   PRIMARY KEY,
    seq          bigint GENERATED ALWAYS AS IDENTITY,
    generation   bigint NOT NULL,
    leased_until timestamptz
);
CREATE INDEX search_queue_seq ON search_queue (seq);

-- One row per store: `id` is always true.
CREATE TABLE search_settings (
    id           boolean     PRIMARY KEY DEFAULT true CHECK (id),
    version      bigint      NOT NULL,
    split_by     text[]      NOT NULL,
    facet_keys   text[]      NOT NULL,
    -- Aligned with `facet_keys`; NULL where the merchant set no label.
    facet_labels text[]      NOT NULL,
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now()
);
