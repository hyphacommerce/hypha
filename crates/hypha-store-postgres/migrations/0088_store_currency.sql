-- The store's base currency: every cost (purchasing, stock value, an order line's cost)
-- is kept in it.
ALTER TABLE stores ADD COLUMN currency text;

-- A store whose markets all charge in one currency keeps its costs in that one; any
-- other store chooses before it records a cost.
UPDATE stores SET currency = (SELECT min(currency) FROM markets)
WHERE (SELECT count(DISTINCT currency) FROM markets) = 1;
