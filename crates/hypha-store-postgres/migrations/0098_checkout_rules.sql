-- The merchant's checkout rules. Where a rule applies, its conditions and its outcome are
-- JSON in the API's shape, read whole with the rule (at most 100 rules, read on every
-- read of a cart); `position` orders them, renumbered together when one moves.
CREATE TABLE checkout_rules (
    id         uuid        CONSTRAINT checkout_rules_pkey PRIMARY KEY,
    version    bigint      NOT NULL CHECK (version > 0),
    name       text        NOT NULL CHECK (length(btrim(name)) > 0),
    enabled    boolean     NOT NULL,
    position   integer     NOT NULL CHECK (position >= 0),
    applies_to jsonb       NOT NULL,
    conditions jsonb       NOT NULL,
    outcome    jsonb       NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX checkout_rules_position ON checkout_rules (position, id);

CREATE TRIGGER hypha_note_write AFTER INSERT OR UPDATE OR DELETE ON checkout_rules
    FOR EACH ROW WHEN (current_setting('hypha.log_writes', true) = 'on')
    EXECUTE FUNCTION hypha_note_write('id');
