-- A cart's shipping address and its choice for its one group of shippable lines
-- (design note 0015, SH8, SH9). One group per cart for now, so these are columns; split
-- shipments will move the choice into a table of groups.
ALTER TABLE carts
    ADD COLUMN shipping_first_name  text,
    ADD COLUMN shipping_last_name   text,
    ADD COLUMN shipping_company     text,
    ADD COLUMN shipping_line1       text,
    ADD COLUMN shipping_line2       text,
    ADD COLUMN shipping_city        text,
    ADD COLUMN shipping_province    text,
    ADD COLUMN shipping_postal_code text,
    ADD COLUMN shipping_country     text,
    ADD COLUMN shipping_phone       text,
    -- No foreign key to shipping options: another module (design note 0002, section 7).
    ADD COLUMN shipping_option_id   uuid,
    -- A provider's quote, as the buyer was shown it.
    ADD COLUMN quote_amount         bigint CHECK (quote_amount BETWEEN 0 AND 9007199254740991),
    ADD COLUMN quote_currency       text CHECK (quote_currency ~ '^[A-Z]{3}$'),
    ADD COLUMN quote_provider       text,
    ADD COLUMN quote_service        text,
    ADD COLUMN quote_reference      text,
    ADD COLUMN quoted_at            timestamptz,
    ADD CONSTRAINT cart_shipping_address_whole CHECK (
        (shipping_last_name IS NULL) = (shipping_line1 IS NULL)
        AND (shipping_line1 IS NULL) = (shipping_city IS NULL)
        AND (shipping_city IS NULL) = (shipping_country IS NULL)
    ),
    ADD CONSTRAINT cart_quote_whole CHECK (
        (quote_amount IS NULL) = (quote_currency IS NULL)
        AND (quote_currency IS NULL) = (quote_provider IS NULL)
        AND (quote_provider IS NULL) = (quote_service IS NULL)
        AND (quote_service IS NULL) = (quoted_at IS NULL)
        AND (quote_amount IS NULL OR shipping_option_id IS NOT NULL)
    );
