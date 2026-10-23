-- The client ID a token pair was issued to, which a refresh must name. Connected agents'
-- tokens take their agent's client; apps' older tokens keep none (their refresh checks
-- the installed manifest's URL).
ALTER TABLE oauth_refresh_tokens ADD COLUMN client_id text;
UPDATE oauth_refresh_tokens t
    SET client_id = a.client_id
    FROM agents a
    WHERE t.agent_id = a.id;
