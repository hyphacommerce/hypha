-- A manifest refresh that asks for new scopes (design note 0029, §4.2) makes an
-- authorization request Hypha started itself: no app waits with a PKCE verifier, so it
-- has no challenge, and approving it changes the grant without issuing a code.
ALTER TABLE oauth_requests ALTER COLUMN code_challenge DROP NOT NULL;
