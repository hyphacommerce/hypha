-- The languages a store's content is translated into beside `default_language`, in the
-- merchant's order: [{"language": "de", "published": true}]. Never the default one.
ALTER TABLE stores ADD COLUMN languages jsonb NOT NULL DEFAULT '[]';

-- The languages a market offers shoppers, in order (the first is its default), each the
-- store's default or a published one. Empty: the store's default alone.
ALTER TABLE markets ADD COLUMN languages text[] NOT NULL DEFAULT '{}';
