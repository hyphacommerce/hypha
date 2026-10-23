-- Approving claims a proposal first: `applying` (its approver and when, in `decided_by`
-- and `decided_at`) until its write's answer is recorded, so nobody else decides it
-- meanwhile. An approval that died leaves it applying; past a lease it counts as pending,
-- so the expiry job looks at those too.
ALTER TABLE agent_proposals
    DROP CONSTRAINT agent_proposals_status_check,
    ADD CONSTRAINT agent_proposals_status_check
        CHECK (status IN ('pending', 'applying', 'applied', 'rejected', 'expired', 'failed'));
DROP INDEX agent_proposals_due;
CREATE INDEX agent_proposals_due ON agent_proposals (expires_at)
    WHERE status IN ('pending', 'applying');
