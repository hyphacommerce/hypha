-- Inventory items across products, lowest available first: each item's stock per
-- location (on hand, reserved, available) and its total available, kept by the adapter
-- with every write of the item's levels and reservations, so a list sorts and filters by
-- index instead of adding up every item's reservations per request.

ALTER TABLE inventory_items ADD COLUMN available bigint NOT NULL DEFAULT 0;

-- One row per location with a level or a reservation, as `InventoryItem::summary` lists
-- them. Derived from inventory_levels and inventory_reservations; never written alone.
CREATE TABLE inventory_stock (
    item_id     uuid   NOT NULL REFERENCES inventory_items (id) ON DELETE CASCADE,
    location_id uuid   NOT NULL,
    on_hand     bigint NOT NULL,
    reserved    bigint NOT NULL,
    available   bigint NOT NULL,
    PRIMARY KEY (item_id, location_id)
);

INSERT INTO inventory_stock (item_id, location_id, on_hand, reserved, available)
SELECT item_id, location_id, sum(on_hand)::bigint, sum(reserved)::bigint,
       (sum(on_hand) - sum(reserved))::bigint
FROM (
    SELECT item_id, location_id, on_hand, 0::bigint AS reserved FROM inventory_levels
    UNION ALL
    SELECT item_id, location_id, 0, quantity FROM inventory_reservations
) AS t
GROUP BY item_id, location_id;

UPDATE inventory_items i
SET available = s.available
FROM (
    SELECT item_id,
           LEAST(GREATEST(sum(available), -9223372036854775808), 9223372036854775807)::bigint
               AS available
    FROM inventory_stock GROUP BY item_id
) AS s
WHERE s.item_id = i.id;

-- The list's two orders: across locations, and at one location.
CREATE INDEX inventory_items_by_available ON inventory_items (available, id);
CREATE INDEX inventory_stock_by_location ON inventory_stock (location_id, available, item_id);
