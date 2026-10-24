-- Files a client uploads straight to storage for the server side to read back once (a CSV
-- of prices to import), kept a day and then swept with their files. Written once and
-- deleted, never updated: no version.
CREATE TABLE temporary_uploads (
    id         uuid        PRIMARY KEY,
    purpose    text        NOT NULL CONSTRAINT temporary_upload_purpose
        CHECK (purpose IN ('price_import')),
    filename   text        NOT NULL,
    byte_size  bigint      NOT NULL CHECK (byte_size > 0),
    checksum   bytea       CHECK (octet_length(checksum) = 32),
    created_at timestamptz NOT NULL,
    expires_at timestamptz NOT NULL
);
CREATE INDEX temporary_uploads_expiry ON temporary_uploads (expires_at, id);
