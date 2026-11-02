-- Unit costs, in the store's base currency, kept in 1/10,000 of its minor unit (as
-- `UnitCost`: `350` is 0.035 cents) beside the currency's code.

-- An item's moving average.
ALTER TABLE inventory_items
    ADD COLUMN unit_cost          bigint,
    ADD COLUMN unit_cost_currency text,
    ADD CONSTRAINT inventory_items_unit_cost
        CHECK ((unit_cost IS NULL) = (unit_cost_currency IS NULL));

-- What a movement's units cost, and the item's cost after it. One currency for both: the
-- store's can't change once a cost is recorded.
ALTER TABLE stock_movements
    ADD COLUMN unit_cost     bigint,
    ADD COLUMN average_after bigint,
    ADD COLUMN cost_currency text,
    ADD CONSTRAINT stock_movements_cost
        CHECK ((cost_currency IS NULL) = (unit_cost IS NULL AND average_after IS NULL));

-- Whether any cost was ever recorded, without reading the whole ledger.
CREATE INDEX stock_movements_costs ON stock_movements (position)
    WHERE cost_currency IS NOT NULL;

-- What a line's units cost when it was placed; none for lines placed before.
ALTER TABLE order_lines
    ADD COLUMN unit_cost          bigint,
    ADD COLUMN unit_cost_currency text,
    ADD CONSTRAINT order_lines_unit_cost
        CHECK ((unit_cost IS NULL) = (unit_cost_currency IS NULL));
