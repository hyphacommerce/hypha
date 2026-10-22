-- The sales report reads a period's orders by when they were placed.
CREATE INDEX orders_by_placed_at ON orders (placed_at);
