-- Lists sorted by more than creation (design note 0024, AU2), one index per sort key
-- with the ID.

-- Orders by total: totals compare within one currency, which the sort requires as a
-- filter, so the index leads with it.
CREATE INDEX orders_by_total ON orders (currency, total, id);

-- The media library sorted by file name, compared like product titles (0044).
CREATE INDEX assets_by_name ON assets ((lower(filename) COLLATE "C"), id);

-- Customers sorted by email or name (first, a space, last), compared the same way.
CREATE INDEX customers_by_email ON customers ((lower(email) COLLATE "C"), id);
CREATE INDEX customers_by_name ON customers
    ((lower(coalesce(first_name, '') || ' ' || coalesce(last_name, '')) COLLATE "C"), id);
