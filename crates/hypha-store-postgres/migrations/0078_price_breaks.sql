-- Quantity prices on a fixed price: from `min_quantity` of the variant in one line,
-- each costs `amount` (in the fixed price's currency). They go with their fixed price.
CREATE TABLE price_list_price_breaks (
    price_list_id uuid    NOT NULL,
    variant_id    uuid    NOT NULL,
    min_quantity  integer NOT NULL CHECK (min_quantity >= 2),
    amount        bigint  NOT NULL CHECK (amount >= 0 AND amount <= 9007199254740991),
    PRIMARY KEY (price_list_id, variant_id, min_quantity),
    FOREIGN KEY (price_list_id, variant_id)
        REFERENCES price_list_prices (price_list_id, variant_id) ON DELETE CASCADE
);
