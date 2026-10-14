-- Product lists filtered by status (design note 0012): one range scan, newest first.
-- Unfiltered lists and order lists use the primary keys (UUIDv7, so newest first).
CREATE INDEX products_by_status ON products (status, id);
