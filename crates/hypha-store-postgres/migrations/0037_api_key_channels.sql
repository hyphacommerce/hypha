-- A publishable key sells through one sales channel (design note 0020, CH4); a secret
-- key has none. Keys made before channels sell through the Online Store (0036). The
-- channel isn't a foreign key: channels aren't deleted, and creating a key checks it.
ALTER TABLE api_keys ADD COLUMN channel_id uuid;
UPDATE api_keys SET channel_id = (SELECT id FROM channels ORDER BY created_at LIMIT 1)
WHERE kind = 'publishable';
ALTER TABLE api_keys ADD CONSTRAINT api_key_channel
    CHECK ((kind = 'publishable') = (channel_id IS NOT NULL));
