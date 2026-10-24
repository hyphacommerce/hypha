-- "Was" prices, struck through beside a price: a variant's own (in its base price's
-- currency) and a fixed price's (in its currency). Each is above the price it's shown
-- beside.
ALTER TABLE variants
    ADD COLUMN compare_at_amount bigint
        CONSTRAINT variant_compare_at
            CHECK (compare_at_amount > price_amount AND compare_at_amount <= 9007199254740991);

ALTER TABLE price_list_prices
    ADD COLUMN compare_at bigint
        CONSTRAINT price_list_price_compare_at
            CHECK (compare_at > amount AND compare_at <= 9007199254740991);

-- What an order line was compared against and which price list priced it, copied at
-- checkout for emails and reports; lines placed before have neither. No foreign key: a
-- price list may be deleted while orders keep its ID.
ALTER TABLE order_lines
    ADD COLUMN compare_at_amount bigint
        CONSTRAINT order_line_compare_at CHECK (compare_at_amount <= 9007199254740991),
    ADD COLUMN price_list_id uuid;
