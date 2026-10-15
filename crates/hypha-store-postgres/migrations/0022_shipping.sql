-- Shipping zones, their options and rates, and the external apps that calculate rates
-- (the hypha-shipping crate, design note 0015). Constraint names are part of the
-- adapter contract: error.rs maps them to `hypha_shipping::keys`.

CREATE TABLE shipping_zones (
    id      uuid   CONSTRAINT shipping_zone_id PRIMARY KEY,
    version bigint NOT NULL CHECK (version > 0),
    name    text   NOT NULL
);

-- A country is in at most one zone: the primary key is the rule.
CREATE TABLE shipping_zone_countries (
    country  text    CONSTRAINT shipping_zone_country PRIMARY KEY CHECK (country ~ '^[A-Z]{2}$'),
    zone_id  uuid    NOT NULL REFERENCES shipping_zones (id) ON DELETE CASCADE,
    position integer NOT NULL,
    UNIQUE (zone_id, position)
);

-- An option's rate is a provider's service, or a built-in basis with tables in
-- shipping_option_tiers. No foreign key to tax classes: another module.
CREATE TABLE shipping_options (
    id                uuid     PRIMARY KEY,
    zone_id           uuid     NOT NULL REFERENCES shipping_zones (id) ON DELETE CASCADE,
    position          integer  NOT NULL,
    name              text     NOT NULL,
    code              text     NOT NULL,
    rate_basis        text     CHECK (rate_basis IN ('flat', 'subtotal', 'weight')),
    provider_key      text,
    provider_service  text,
    estimate_min_days integer  CHECK (estimate_min_days BETWEEN 0 AND 65535),
    estimate_max_days integer  CHECK (estimate_max_days BETWEEN 0 AND 65535),
    tax_class_id      uuid,
    is_backup         boolean  NOT NULL,
    enabled           boolean  NOT NULL,
    UNIQUE (zone_id, position),
    UNIQUE (zone_id, code),
    CONSTRAINT shipping_option_rate CHECK (
        (rate_basis IS NULL) = (provider_key IS NOT NULL)
        AND (provider_key IS NULL) = (provider_service IS NULL)
    ),
    CONSTRAINT shipping_option_estimate CHECK (
        (estimate_min_days IS NULL) = (estimate_max_days IS NULL)
    )
);

CREATE INDEX shipping_options_zone ON shipping_options (zone_id);

-- A built-in rate's tables, in their order, each with its tiers in order.
CREATE TABLE shipping_option_tiers (
    option_id      uuid    NOT NULL REFERENCES shipping_options (id) ON DELETE CASCADE,
    table_position integer NOT NULL,
    position       integer NOT NULL,
    currency       text    NOT NULL CHECK (currency ~ '^[A-Z]{3}$'),
    -- u64 in the domain; numeric holds all of it.
    up_to          numeric(20, 0) CHECK (up_to >= 0),
    amount         bigint  NOT NULL CHECK (amount BETWEEN 0 AND 9007199254740991),
    PRIMARY KEY (option_id, table_position, position)
);

CREATE TABLE shipping_app_providers (
    key          text    CONSTRAINT shipping_provider_key PRIMARY KEY,
    version      bigint  NOT NULL CHECK (version > 0),
    name         text    NOT NULL,
    callback_url text    NOT NULL,
    timeout_ms   integer NOT NULL CHECK (timeout_ms BETWEEN 100 AND 10000),
    -- Sealed under HYPHA_SECRET_KEY, the key as context (like TOTP secrets, 0008).
    secret       bytea   NOT NULL,
    enabled      boolean NOT NULL
);

CREATE TABLE shipping_app_provider_services (
    provider_key text    NOT NULL REFERENCES shipping_app_providers (key) ON DELETE CASCADE,
    position     integer NOT NULL,
    code         text    NOT NULL,
    name         text    NOT NULL,
    PRIMARY KEY (provider_key, position),
    UNIQUE (provider_key, code)
);
