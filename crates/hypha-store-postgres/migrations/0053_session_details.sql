-- What a staff member sees when listing their sessions (design note 0008, §9): where
-- the login came from and when the session was last used. `last_used_at` is written
-- at most once per `Session::USE_RESOLUTION`, without moving the version.
ALTER TABLE sessions
    ADD COLUMN client_ip    text CHECK (char_length(client_ip) <= 64),
    ADD COLUMN user_agent   text CHECK (char_length(user_agent) <= 512),
    ADD COLUMN last_used_at timestamptz;

UPDATE sessions SET last_used_at = created_at;

ALTER TABLE sessions ALTER COLUMN last_used_at SET NOT NULL;
