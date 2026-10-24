-- Discounts: automatic or by code. Their conditions and rewards are one JSON document
-- (`rules`), read whole with the discount; `classes` is derived from the rewards and kept
-- apart so lists can filter by it.
CREATE TABLE discounts (
    id                uuid        CONSTRAINT discounts_pkey PRIMARY KEY,
    version           bigint      NOT NULL CHECK (version > 0),
    title             text        NOT NULL CHECK (length(btrim(title)) > 0),
    method            text        NOT NULL CHECK (method IN ('automatic', 'codes')),
    status            text        NOT NULL CHECK (status IN ('draft', 'active')),
    starts_at         timestamptz,
    ends_at           timestamptz,
    classes           text[]      NOT NULL,
    combines_with     text[]      NOT NULL,
    rules             jsonb       NOT NULL,
    usage_limit       integer     CHECK (usage_limit > 0),
    once_per_customer boolean     NOT NULL,
    created_at        timestamptz NOT NULL DEFAULT now(),
    updated_at        timestamptz NOT NULL DEFAULT now()
);
-- What every cart reads: the active automatic ones.
CREATE INDEX discounts_live_automatic ON discounts (id)
    WHERE method = 'automatic' AND status = 'active';

-- Uses so far, one row per discount from its creation: kept apart from the discount so
-- concurrent checkouts counting a popular discount never fight over its version. Written
-- only by checkout's batch, under the limit (`UPDATE … WHERE used < limit`).
CREATE TABLE discount_usage_counts (
    discount_id uuid   PRIMARY KEY REFERENCES discounts (id) ON DELETE CASCADE,
    used        bigint NOT NULL DEFAULT 0 CHECK (used >= 0)
);

-- Codes, unique in the store by their normalised text; `used` moves like the counter.
CREATE TABLE discount_codes (
    normalized  text        CONSTRAINT discount_code PRIMARY KEY,
    discount_id uuid        NOT NULL REFERENCES discounts (id) ON DELETE CASCADE,
    code        text        NOT NULL,
    usage_limit integer     CHECK (usage_limit > 0),
    active      boolean     NOT NULL DEFAULT true,
    batch       text,
    used        bigint      NOT NULL DEFAULT 0 CHECK (used >= 0),
    created_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX discount_codes_by_discount ON discount_codes (discount_id, normalized);

-- The orders each discount was used on. Orders are never deleted and customers with
-- orders never are either, so neither is a foreign key.
CREATE TABLE discount_usages (
    discount_id       uuid        NOT NULL REFERENCES discounts (id) ON DELETE CASCADE,
    order_id          uuid        NOT NULL,
    customer_id       uuid        NOT NULL,
    code              text,
    -- What the discount took off the order, in its currency.
    amount            bigint      NOT NULL CHECK (amount >= 0),
    currency          text        NOT NULL,
    once_per_customer boolean     NOT NULL,
    used_at           timestamptz NOT NULL,
    PRIMARY KEY (discount_id, order_id)
);
-- Once per customer: a second use by the same customer fails the checkout's batch.
CREATE UNIQUE INDEX discount_per_customer ON discount_usages (discount_id, customer_id)
    WHERE once_per_customer;
CREATE INDEX discount_usages_by_customer ON discount_usages (customer_id);

CREATE TRIGGER hypha_note_write AFTER INSERT OR UPDATE OR DELETE ON discounts
    FOR EACH ROW WHEN (current_setting('hypha.log_writes', true) = 'on')
    EXECUTE FUNCTION hypha_note_write('id');
