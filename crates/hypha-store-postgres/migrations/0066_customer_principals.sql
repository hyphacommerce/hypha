-- Customers sign in too (design note 0034): identities and sessions may belong to a
-- `customer` principal. The checks were written for staff alone.
ALTER TABLE identities DROP CONSTRAINT identities_principal_kind_check;
ALTER TABLE identities
    ADD CONSTRAINT identities_principal_kind_check
    CHECK (principal_kind IN ('staff', 'customer'));
ALTER TABLE sessions DROP CONSTRAINT sessions_principal_kind_check;
ALTER TABLE sessions
    ADD CONSTRAINT sessions_principal_kind_check
    CHECK (principal_kind IN ('staff', 'customer'));
