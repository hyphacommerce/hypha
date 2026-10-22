-- MCP clients connect through the same OAuth flow as apps: their client ID is the URL of
-- an OAuth Client ID Metadata Document, approving makes a connected agent, and its codes
-- and tokens belong to that agent instead of an app.

-- A request is an app's (its key and manifest) or an MCP client's (its metadata).
ALTER TABLE oauth_requests
    ALTER COLUMN app_key DROP NOT NULL,
    ALTER COLUMN manifest DROP NOT NULL,
    ADD COLUMN client_metadata jsonb,
    ADD CONSTRAINT oauth_request_client CHECK (
        (app_key IS NOT NULL AND manifest IS NOT NULL AND client_metadata IS NULL)
        OR (app_key IS NULL AND manifest IS NULL AND client_metadata IS NOT NULL)
    );

-- A code is for an app or for an agent. No foreign key, as for apps: the agent is staged
-- in the same batch, and a code lives a minute.
ALTER TABLE oauth_codes
    ALTER COLUMN app_key DROP NOT NULL,
    ADD COLUMN agent_id uuid,
    ADD CONSTRAINT oauth_code_client CHECK ((app_key IS NULL) <> (agent_id IS NULL));

-- A token pair belongs to an app or to an agent, and goes with it.
ALTER TABLE oauth_refresh_tokens
    ALTER COLUMN app_key DROP NOT NULL,
    ADD COLUMN agent_id uuid REFERENCES agents (id) ON DELETE CASCADE,
    ADD CONSTRAINT oauth_token_client CHECK ((app_key IS NULL) <> (agent_id IS NULL));
CREATE INDEX oauth_refresh_tokens_agent ON oauth_refresh_tokens (agent_id)
    WHERE agent_id IS NOT NULL;
