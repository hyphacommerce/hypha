-- Fulfilments (the hypha-fulfillment crate, design note 0018). No foreign keys to orders,
-- order lines or locations: separate aggregates, linked by ID (an order's lines are
-- rewritten on every update, so a reference to them couldn't hold anyway).
CREATE TABLE fulfillments (
    id           uuid        CONSTRAINT fulfillments_pkey PRIMARY KEY,
    version      bigint      NOT NULL CHECK (version > 0),
    order_id     uuid        NOT NULL,
    location_id  uuid        NOT NULL,
    -- Set once it's cancelled.
    cancelled_at timestamptz,
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now()
);

-- An order's fulfilments, newest first.
CREATE INDEX fulfillments_by_order ON fulfillments (order_id, id DESC);

-- Written once, with the fulfilment.
CREATE TABLE fulfillment_lines (
    fulfillment_id uuid    NOT NULL REFERENCES fulfillments (id) ON DELETE CASCADE,
    position       integer NOT NULL,
    order_line_id  uuid    NOT NULL,
    quantity       bigint  NOT NULL CHECK (quantity BETWEEN 1 AND 4294967295),
    PRIMARY KEY (fulfillment_id, position),
    UNIQUE (fulfillment_id, order_line_id)
);

-- Replaced whenever the tracking changes.
CREATE TABLE fulfillment_tracking (
    fulfillment_id uuid    NOT NULL REFERENCES fulfillments (id) ON DELETE CASCADE,
    position       integer NOT NULL,
    carrier        text,
    number         text    NOT NULL,
    url            text,
    PRIMARY KEY (fulfillment_id, position)
);
