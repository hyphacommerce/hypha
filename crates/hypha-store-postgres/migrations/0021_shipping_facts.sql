-- Facts shipping reads (design note 0015, SH11): a variant's weight and whether it is
-- shipped at all, and the address a location ships from.

-- Grams, a u32 in the domain (`Weight`); NULL when not entered.
ALTER TABLE variants
    ADD COLUMN weight_grams bigint CHECK (weight_grams BETWEEN 0 AND 4294967295),
    ADD COLUMN requires_shipping boolean NOT NULL DEFAULT true;

-- The `Address` columns customers use, all NULL when a location has no address; the
-- domain's rules decide which are required once one is set.
ALTER TABLE locations
    ADD COLUMN address_first_name  text,
    ADD COLUMN address_last_name   text,
    ADD COLUMN address_company     text,
    ADD COLUMN address_line1       text,
    ADD COLUMN address_line2       text,
    ADD COLUMN address_city        text,
    ADD COLUMN address_province    text,
    ADD COLUMN address_postal_code text,
    ADD COLUMN address_country     text,
    ADD COLUMN address_phone       text,
    ADD CONSTRAINT location_address_whole CHECK (
        (address_last_name IS NULL) = (address_line1 IS NULL)
        AND (address_line1 IS NULL) = (address_city IS NULL)
        AND (address_city IS NULL) = (address_country IS NULL)
    );
