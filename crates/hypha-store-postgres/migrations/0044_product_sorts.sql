-- Product lists sorted by title or by last change (design note 0024, AU2): one index
-- per sort key with the ID, which breaks ties, so a page stays one range scan. Titles
-- compare lowercased, by bytes (`COLLATE "C"`), as `SortKey::text` does in Rust.
CREATE INDEX products_by_title ON products ((lower(title) COLLATE "C"), id);
CREATE INDEX products_by_updated ON products (updated_at, id);
