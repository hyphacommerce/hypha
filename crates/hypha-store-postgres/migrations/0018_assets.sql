-- Media assets (design note 0013): one file and its metadata. `kind` is derived from
-- `mime_type` and stored only so the library can filter by it with an index.
CREATE TABLE assets (
    id           uuid        CONSTRAINT assets_pkey PRIMARY KEY,
    version      bigint      NOT NULL CHECK (version > 0),
    status       text        NOT NULL CHECK (status IN ('pending', 'ready', 'failed')),
    kind         text        NOT NULL CHECK (kind IN ('image', 'video', 'file')),
    filename     text        NOT NULL,
    blob_key     text        NOT NULL,
    file_version integer     NOT NULL CHECK (file_version > 0),
    mime_type    text        NOT NULL,
    byte_size    bigint      NOT NULL CHECK (byte_size > 0),
    checksum     bytea       CHECK (octet_length(checksum) = 32),
    -- u32 in the domain, so bigint here.
    width        bigint      CHECK (width BETWEEN 0 AND 4294967295),
    height       bigint      CHECK (height BETWEEN 0 AND 4294967295),
    alt          text,
    title        text,
    visibility   text        NOT NULL CHECK (visibility IN ('public', 'private')),
    created_at   timestamptz NOT NULL,
    updated_at   timestamptz NOT NULL
);

-- The library: ready assets, newest first, optionally of one kind (design note 0012).
CREATE INDEX assets_library ON assets (status, id);
CREATE INDEX assets_library_by_kind ON assets (status, kind, id);
