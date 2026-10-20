-- Custom fields (design note 0033): definitions as data, and values one row per
-- (definition, owner), outside the owners' tables (CF5).
CREATE TABLE custom_field_definitions (
    id                uuid        CONSTRAINT custom_field_definitions_pkey PRIMARY KEY,
    version           bigint      NOT NULL CHECK (version > 0),
    owner_type        text        NOT NULL
                                  CHECK (owner_type IN ('product', 'variant', 'category',
                                                        'customer', 'order')),
    namespace         text        NOT NULL
                                  CHECK (namespace = 'custom'
                                         OR (namespace ~ '^(app|spore)_[a-z][a-z0-9_]*$'
                                             AND length(namespace) <= 64)),
    key               text        NOT NULL CHECK (key ~ '^[a-z][a-z0-9_]{1,63}$'),
    -- The app or spore that manages an `app_*` / `spore_*` namespace (its key, the same
    -- text); NULL for `custom`. Removing the app removes its fields and their values
    -- (design note 0033, §8), like `shipping_provider_app`.
    app               text        CONSTRAINT custom_field_definition_app
                                  REFERENCES apps (key) ON DELETE CASCADE,
    name              text        NOT NULL CHECK (length(btrim(name)) > 0),
    description       text,
    -- `text`, `list.money`, …: `FieldType`'s spelling.
    field_type        text        NOT NULL,
    -- `RawValidations` as JSON.
    validations       jsonb       NOT NULL,
    admin_access      text        NOT NULL CHECK (admin_access IN ('read', 'read_write')),
    storefront_access text        NOT NULL CHECK (storefront_access IN ('none', 'read')),
    private           boolean     NOT NULL,
    personal          boolean     NOT NULL,
    filterable        boolean     NOT NULL,
    unique_values     boolean     NOT NULL,
    -- Dense within an owner type, renumbered in the same batch as a move; not a key.
    position          integer     NOT NULL CHECK (position >= 0),
    created_at        timestamptz NOT NULL DEFAULT now(),
    updated_at        timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT custom_field_key UNIQUE (owner_type, namespace, key),
    CHECK ((namespace = 'custom') = (app IS NULL)),
    CHECK (app IS NULL OR app = namespace)
);
CREATE INDEX custom_field_definitions_order
    ON custom_field_definitions (owner_type, position, id);
CREATE INDEX custom_field_definitions_app ON custom_field_definitions (app);

-- Values: typed JSON plus the columns filters and `unique` read, derived by the domain
-- (`ValueRow::new`). No foreign key to the owner, whose table is another module's: the
-- owner's deletion event clears them (§6.3).
CREATE TABLE custom_field_values (
    definition_id   uuid        NOT NULL
                                CONSTRAINT custom_field_value_definition
                                REFERENCES custom_field_definitions (id) ON DELETE CASCADE,
    owner_id        uuid        NOT NULL,
    -- A variant's product, so events about it name the product; NULL otherwise.
    owner_parent_id uuid,
    value           jsonb       NOT NULL,
    -- Byte order, as the domain compares text (lowercased already).
    scalar_text     text        COLLATE "C",
    scalar_num      numeric,
    -- Only for `unique` definitions; NULLs never clash.
    unique_text     text        COLLATE "C",
    version         bigint      NOT NULL CHECK (version > 0),
    created_at      timestamptz NOT NULL DEFAULT now(),
    updated_at      timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT custom_field_values_pkey PRIMARY KEY (definition_id, owner_id),
    CONSTRAINT custom_field_unique_value UNIQUE (definition_id, unique_text)
);
-- An owner's (or a page of owners') fields.
CREATE INDEX custom_field_values_owner ON custom_field_values (owner_id);
-- List `contains` filters (`value @> '["vip"]'`). A plain GIN rather than btree_gin's
-- (definition_id, value): extensions are database-wide while each test runs in its own
-- schema, and managed databases may not allow one; the filter's EXISTS also probes the
-- primary key per owner.
CREATE INDEX custom_field_values_value ON custom_field_values USING gin (value jsonb_path_ops);
-- Equality and range filters on single values.
CREATE INDEX custom_field_values_text ON custom_field_values (definition_id, scalar_text)
    WHERE scalar_text IS NOT NULL;
CREATE INDEX custom_field_values_num ON custom_field_values (definition_id, scalar_num)
    WHERE scalar_num IS NOT NULL;
