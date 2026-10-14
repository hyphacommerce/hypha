-- The rules the domain already enforces, repeated in the schema so a manual edit or a
-- buggy adapter can't store what `restore` would reject (review plan, phase 3).
-- Formats rather than closed lists: a new currency or country needs no migration.

ALTER TABLE variants
    ADD CONSTRAINT variant_price_currency CHECK (price_currency ~ '^[A-Z]{3}$'),
    -- Money::MAX_AMOUNT (2^53 - 1), so JSON clients and D1 read every amount exactly.
    ADD CONSTRAINT variant_price_amount CHECK (price_amount <= 9007199254740991);

ALTER TABLE markets
    ADD CONSTRAINT market_currency CHECK (currency ~ '^[A-Z]{3}$');

ALTER TABLE market_countries
    ADD CONSTRAINT market_country_code CHECK (country ~ '^[A-Z]{2}$');

ALTER TABLE price_lists
    ADD CONSTRAINT price_list_currency CHECK (currency ~ '^[A-Z]{3}$');

ALTER TABLE price_list_prices
    ADD CONSTRAINT price_list_price_currency CHECK (currency ~ '^[A-Z]{3}$'),
    ADD CONSTRAINT price_list_price_amount CHECK (amount <= 9007199254740991);

ALTER TABLE tax_regions
    ADD CONSTRAINT tax_region_country_code CHECK (country ~ '^[A-Z]{2}$');

ALTER TABLE carts
    ADD CONSTRAINT cart_country_code CHECK (country ~ '^[A-Z]{2}$'),
    -- Email::new lowercases, so a stored email is always lowercase.
    ADD CONSTRAINT cart_email_lowercase CHECK (email = lower(email));

ALTER TABLE customers
    ADD CONSTRAINT customer_email_lowercase CHECK (email = lower(email));

ALTER TABLE customer_addresses
    ADD CONSTRAINT customer_address_country_code CHECK (country ~ '^[A-Z]{2}$');
