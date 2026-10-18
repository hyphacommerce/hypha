-- Payments (the hypha-payments crate, design note 0027): the methods a store offers,
-- the payments an order has, the method a cart chose, and the order's full set of
-- payment statuses.

-- A method's markets and channels are sets of IDs another domain owns, kept unchecked
-- like a shipping option's tax class: an unknown one never matches. Empty: all of them.
CREATE TABLE payment_methods (
    id           uuid        CONSTRAINT payment_methods_pkey PRIMARY KEY,
    version      bigint      NOT NULL CHECK (version > 0),
    provider     text        NOT NULL,
    name         text        NOT NULL,
    instructions text,
    market_ids   uuid[]      NOT NULL DEFAULT '{}',
    channel_ids  uuid[]      NOT NULL DEFAULT '{}',
    enabled      boolean     NOT NULL,
    position     bigint      NOT NULL CHECK (position BETWEEN 0 AND 4294967295),
    settings     jsonb       NOT NULL DEFAULT '{}',
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now()
);

-- The order shoppers see them in.
CREATE INDEX payment_methods_by_position ON payment_methods (position, id);

-- No foreign keys to orders or methods: separate aggregates, linked by ID. A payment
-- copies its method's name, provider and instructions, so deleting the method leaves it
-- whole. Amounts are minor units of `currency`.
CREATE TABLE payments (
    id           uuid        CONSTRAINT payments_pkey PRIMARY KEY,
    version      bigint      NOT NULL CHECK (version > 0),
    order_id     uuid        NOT NULL,
    method_id    uuid,
    method_name  text,
    provider     text        NOT NULL,
    currency     text        NOT NULL,
    amount       bigint      NOT NULL CHECK (amount BETWEEN 0 AND 9007199254740991),
    refunded     bigint      NOT NULL DEFAULT 0 CHECK (refunded BETWEEN 0 AND amount),
    status       text        NOT NULL CHECK (status IN ('pending', 'authorized', 'paid',
                                 'failed', 'voided', 'partially_refunded', 'refunded')),
    reference    text,
    instructions text,
    paid_at      timestamptz,
    voided_at    timestamptz,
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now()
);

-- An order's payments, newest first.
CREATE INDEX payments_by_order ON payments (order_id, id DESC);
-- Whether a method has an open payment (it can't be deleted then).
CREATE INDEX payments_open_by_method ON payments (method_id)
    WHERE status IN ('pending', 'authorized');

-- Orders marked paid before payments existed get the payment that marking stands for:
-- manual, of the whole total, made and paid when the order last changed. Its ID is a
-- UUIDv7 of that moment, like the ones hosts make, so an order's payments still sort
-- newest first by ID: a random UUID (v4) with its first 48 bits replaced by the Unix
-- time in milliseconds and its version nibble set to 7 (v4's variant bits are v7's).
INSERT INTO payments (id, version, order_id, provider, currency, amount, status, paid_at,
                      created_at, updated_at)
SELECT encode(set_byte(v7.bytes, 6, (get_byte(v7.bytes, 6) & 15) | 112), 'hex')::uuid,
       1, orders.id, 'manual', currency, total, 'paid', updated_at, updated_at, updated_at
FROM orders,
     LATERAL (SELECT overlay(uuid_send(gen_random_uuid())
                             PLACING substring(int8send(floor(extract(epoch FROM updated_at)
                                                              * 1000)::bigint) FROM 3)
                             FROM 1 FOR 6) AS bytes) AS v7
WHERE financial_status = 'paid';

-- The method the shopper chose (checked again at checkout).
ALTER TABLE carts ADD COLUMN payment_method_id uuid;

-- The full set of an order's payment statuses (PA7).
ALTER TABLE orders DROP CONSTRAINT orders_financial_status_check;
ALTER TABLE orders ADD CONSTRAINT orders_financial_status_check
    CHECK (financial_status IN ('pending', 'authorized', 'partially_paid', 'paid',
                                'partially_refunded', 'refunded', 'voided'));
