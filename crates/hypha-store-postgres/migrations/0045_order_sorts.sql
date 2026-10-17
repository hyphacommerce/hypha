-- Order lists sorted by total (design note 0024, AU2): totals compare within one
-- currency, which the sort requires as a filter, so the index leads with it.
CREATE INDEX orders_by_total ON orders (currency, total, id);
