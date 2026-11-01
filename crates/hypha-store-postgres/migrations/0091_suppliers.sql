-- Suppliers the store buys stock from. Contacts are a JSON list read whole with the
-- supplier (at most 10); the address uses the `Address` columns stores and locations use,
-- all NULL for none.
CREATE TABLE suppliers (
    id                  uuid        CONSTRAINT suppliers_pkey PRIMARY KEY,
    version             bigint      NOT NULL CHECK (version > 0),
    name                text        NOT NULL CHECK (length(btrim(name)) > 0),
    contacts            jsonb       NOT NULL DEFAULT '[]',
    address_first_name  text,
    address_last_name   text,
    address_company     text,
    address_line1       text,
    address_line2       text,
    address_city        text,
    address_province    text,
    address_postal_code text,
    address_country     text CHECK (address_country ~ '^[A-Z]{2}$'),
    address_phone       text,
    currency            text        NOT NULL,
    -- When it's paid: `in_advance`, `on_receipt`, or `net` with its days.
    payment_terms_due   text        CHECK (payment_terms_due IN ('in_advance', 'on_receipt', 'net')),
    payment_terms_days  integer     CHECK (payment_terms_days BETWEEN 1 AND 365),
    payment_terms_note  text,
    lead_time_days      integer     CHECK (lead_time_days >= 0),
    tax_number          text,
    notes               text,
    archived            boolean     NOT NULL DEFAULT false,
    created_at          timestamptz NOT NULL DEFAULT now(),
    updated_at          timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT supplier_address_whole CHECK (
        (address_last_name IS NULL) = (address_line1 IS NULL)
        AND (address_line1 IS NULL) = (address_city IS NULL)
        AND (address_city IS NULL) = (address_country IS NULL)
    ),
    CONSTRAINT supplier_payment_terms CHECK (
        (payment_terms_due = 'net') = (payment_terms_days IS NOT NULL)
        AND (payment_terms_due IS NOT NULL OR payment_terms_note IS NULL)
    )
);

CREATE TRIGGER hypha_note_write AFTER INSERT OR UPDATE OR DELETE ON suppliers
    FOR EACH ROW WHEN (current_setting('hypha.log_writes', true) = 'on')
    EXECUTE FUNCTION hypha_note_write('id');

-- What each supplier sells, one row per (supplier, variant), going with the supplier. No
-- key into variants: a removed variant's rows are deleted by its cleanup. The cost is in
-- the supplier's currency, in 1/10,000 of its minor unit.
CREATE TABLE supplier_variants (
    supplier_id        uuid    NOT NULL
                               CONSTRAINT supplier_variant_supplier
                               REFERENCES suppliers (id) ON DELETE CASCADE,
    variant_id         uuid    NOT NULL,
    supplier_sku       text,
    unit_cost          bigint  CHECK (unit_cost >= 0),
    unit_cost_currency text,
    min_order_quantity integer CHECK (min_order_quantity > 0),
    lead_time_days     integer CHECK (lead_time_days >= 0),
    CONSTRAINT supplier_variants_pkey PRIMARY KEY (supplier_id, variant_id),
    CONSTRAINT supplier_variants_unit_cost
        CHECK ((unit_cost IS NULL) = (unit_cost_currency IS NULL))
);

CREATE INDEX supplier_variants_by_variant ON supplier_variants (variant_id);
