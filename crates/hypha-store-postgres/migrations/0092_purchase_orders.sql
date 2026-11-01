-- Purchase orders: what the store orders from a supplier, to one of its locations.
-- Numbers come from their own sequence (atomic, never rolled back), shown `PO-1001`.
CREATE SEQUENCE purchase_order_numbers START 1001;

CREATE TABLE purchase_orders (
    id                 uuid        CONSTRAINT purchase_orders_pkey PRIMARY KEY,
    version            bigint      NOT NULL CHECK (version > 0),
    number             bigint      NOT NULL CHECK (number > 0)
                                   CONSTRAINT purchase_order_number UNIQUE,
    -- A supplier on an order isn't deleted (it's archived instead), so the order keeps it.
    supplier_id        uuid        NOT NULL
                                   CONSTRAINT purchase_order_supplier REFERENCES suppliers (id),
    -- The supplier's name when ordered, for documents.
    supplier_name      text,
    -- Where the goods go; no key, as for stock movements: locations are never deleted.
    destination_id     uuid,
    currency           text        NOT NULL,
    -- One unit of the order's currency in the store's, in 1/100,000,000.
    exchange_rate      bigint      CHECK (exchange_rate > 0),
    payment_terms_due  text        CHECK (payment_terms_due IN ('in_advance', 'on_receipt', 'net')),
    payment_terms_days integer     CHECK (payment_terms_days BETWEEN 1 AND 365),
    payment_terms_note text,
    expected_at        timestamptz,
    supplier_reference text,
    note_to_supplier   text,
    note               text,
    -- In the order's currency.
    tax                bigint      CHECK (tax >= 0),
    status             text        NOT NULL
                                   CHECK (status IN ('draft', 'ordered', 'closed', 'cancelled')),
    receipt_status     text        NOT NULL
                                   CHECK (receipt_status IN ('none', 'partial', 'received')),
    ordered_at         timestamptz,
    sent_at            timestamptz,
    closed_at          timestamptz,
    cancelled_at       timestamptz,
    revision           integer     NOT NULL DEFAULT 0 CHECK (revision >= 0),
    -- What lists show without the lines: written with the order. `total` is NULL past the
    -- largest amount.
    line_count         integer     NOT NULL DEFAULT 0,
    total              bigint,
    created_at         timestamptz NOT NULL DEFAULT now(),
    updated_at         timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT purchase_order_payment_terms CHECK (
        (payment_terms_due = 'net') = (payment_terms_days IS NOT NULL)
        AND (payment_terms_due IS NOT NULL OR payment_terms_note IS NULL)
    )
);

CREATE INDEX purchase_orders_by_supplier ON purchase_orders (supplier_id, number);
CREATE INDEX purchase_orders_by_expected ON purchase_orders (expected_at NULLS LAST, id);

CREATE TRIGGER hypha_note_write AFTER INSERT OR UPDATE OR DELETE ON purchase_orders
    FOR EACH ROW WHEN (current_setting('hypha.log_writes', true) = 'on')
    EXECUTE FUNCTION hypha_note_write('id');

-- An order's lines, one per variant, written whole with the order. No key into variants:
-- the titles and SKU are snapshots, and a removed variant's lines stay.
CREATE TABLE purchase_order_lines (
    purchase_order_id  uuid    NOT NULL REFERENCES purchase_orders (id) ON DELETE CASCADE,
    position           integer NOT NULL,
    variant_id         uuid    NOT NULL,
    product_title      text    NOT NULL,
    variant_title      text,
    sku                text,
    supplier_sku       text,
    quantity           integer NOT NULL CHECK (quantity > 0),
    -- In the order's currency, in 1/10,000 of its minor unit.
    unit_cost          bigint  NOT NULL CHECK (unit_cost >= 0),
    unit_cost_currency text    NOT NULL,
    received           integer NOT NULL DEFAULT 0 CHECK (received >= 0),
    rejected           integer NOT NULL DEFAULT 0 CHECK (rejected >= 0),
    cancelled          integer NOT NULL DEFAULT 0 CHECK (cancelled >= 0),
    CONSTRAINT purchase_order_lines_pkey PRIMARY KEY (purchase_order_id, variant_id)
);

CREATE INDEX purchase_order_lines_by_variant ON purchase_order_lines (variant_id);

-- Extra costs (shipping, duties, fees), in the order's currency or the store's.
CREATE TABLE purchase_order_costs (
    purchase_order_id uuid    NOT NULL REFERENCES purchase_orders (id) ON DELETE CASCADE,
    position          integer NOT NULL,
    kind              text    NOT NULL CHECK (kind IN ('shipping', 'duties', 'fees', 'other')),
    description       text,
    amount            bigint  NOT NULL CHECK (amount >= 0),
    currency          text    NOT NULL,
    allocation        text    NOT NULL CHECK (allocation IN ('by_value', 'by_quantity', 'none')),
    CONSTRAINT purchase_order_costs_pkey PRIMARY KEY (purchase_order_id, position)
);
