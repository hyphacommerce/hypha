-- The store's details (design note 0021, SU4): all optional except the language its
-- content is written in, which is English until the merchant changes it (SU5). The
-- legal address uses the `Address` columns customers and locations use, all NULL for
-- none.

ALTER TABLE stores
    ADD COLUMN contact_email text CHECK (contact_email = lower(contact_email)),
    ADD COLUMN description   text CHECK (char_length(description) <= 500),
    ADD COLUMN legal_name    text,
    ADD COLUMN timezone      text,
    ADD COLUMN default_language text NOT NULL DEFAULT 'en',
    ADD COLUMN legal_address_first_name  text,
    ADD COLUMN legal_address_last_name   text,
    ADD COLUMN legal_address_company     text,
    ADD COLUMN legal_address_line1       text,
    ADD COLUMN legal_address_line2       text,
    ADD COLUMN legal_address_city        text,
    ADD COLUMN legal_address_province    text,
    ADD COLUMN legal_address_postal_code text,
    ADD COLUMN legal_address_country     text CHECK (legal_address_country ~ '^[A-Z]{2}$'),
    ADD COLUMN legal_address_phone       text,
    ADD CONSTRAINT store_legal_address_whole CHECK (
        (legal_address_last_name IS NULL) = (legal_address_line1 IS NULL)
        AND (legal_address_line1 IS NULL) = (legal_address_city IS NULL)
        AND (legal_address_city IS NULL) = (legal_address_country IS NULL)
    );
