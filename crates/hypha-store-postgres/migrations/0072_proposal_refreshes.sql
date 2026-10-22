-- Approving a proposal the store has moved past previews it again: the proposal keeps
-- the new preview and digest, stays pending, and counts how often that happened.
ALTER TABLE agent_proposals
    ADD COLUMN refreshes integer NOT NULL DEFAULT 0 CHECK (refreshes >= 0);
