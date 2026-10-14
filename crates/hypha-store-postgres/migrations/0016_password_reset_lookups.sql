-- Password resets by identity: the latest one (one email every few minutes) and the
-- open ones a reset or password change uses up (design note 0008, section 5).
CREATE INDEX password_resets_by_identity ON password_resets (identity_id, created_at DESC);
