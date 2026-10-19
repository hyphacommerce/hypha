-- Spores are apps of kind spore (design note 0030, SP3): their keys are spore_<name>,
-- beside the external apps' app_<name>. The kind is the key's prefix, so no column.
ALTER TABLE apps DROP CONSTRAINT apps_key_check;
ALTER TABLE apps
    ADD CONSTRAINT apps_key_check
        CHECK (key ~ '^(app|spore)_[a-z][a-z0-9_]*$' AND length(key) <= 64);
