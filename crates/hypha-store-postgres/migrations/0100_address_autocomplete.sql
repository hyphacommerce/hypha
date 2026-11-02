-- The app suggesting addresses while someone types one (`address.suggest` and
-- `address.resolve`).
ALTER TABLE checkout_settings ADD COLUMN autocomplete_app text;
