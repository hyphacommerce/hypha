-- The store's checkout settings: one row, like search's.
CREATE TABLE checkout_settings (
    id               boolean     CONSTRAINT checkout_settings_pkey PRIMARY KEY DEFAULT true
                                 CHECK (id),
    version          bigint      NOT NULL CHECK (version > 0),
    -- The app verifying shipping addresses (`address.verify`).
    verification_app text,
    created_at       timestamptz NOT NULL DEFAULT now(),
    updated_at       timestamptz NOT NULL DEFAULT now()
);

CREATE TRIGGER hypha_note_write AFTER INSERT OR UPDATE OR DELETE ON checkout_settings
    FOR EACH ROW WHEN (current_setting('hypha.log_writes', true) = 'on')
    EXECUTE FUNCTION hypha_note_write('id');

-- A cart's shipping address as verified (the digest of what was sent, the app's answer,
-- who answered and when), so the same address is never looked up twice.
ALTER TABLE carts ADD COLUMN address_check jsonb;

-- An order's address check: the verification copied from its cart (or asked again when
-- staff edit the address), the warnings it was placed with, and who marked it valid.
-- `address_review` is derived from it on every write, for the "needs review" filter.
ALTER TABLE orders ADD COLUMN address_check jsonb;
ALTER TABLE orders ADD COLUMN address_review boolean NOT NULL DEFAULT false;

CREATE INDEX orders_address_review ON orders (id DESC) WHERE address_review;
