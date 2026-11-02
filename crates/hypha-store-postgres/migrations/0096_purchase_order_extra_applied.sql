-- What a purchase order line's received stock carries of the order's extra costs, in the
-- store's currency in 1/10,000 of its minor unit, either sign: a later change to the
-- extra costs is measured against it, so only the difference reaches the stock. Lines
-- received before it was kept have none, and are measured against the order as it was
-- before the change.
ALTER TABLE purchase_order_lines ADD COLUMN extra_applied bigint;
