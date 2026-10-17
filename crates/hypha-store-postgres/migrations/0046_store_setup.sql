-- How far the merchant got through the admin's setup guide (design note 0024, AU11):
-- the step they were last on, and when they finished it or put it away.
ALTER TABLE stores
    ADD COLUMN setup_step text,
    ADD COLUMN setup_finished_at timestamptz;

-- A store already selling (a market and a product) is past setting up: don't show it
-- the guide.
UPDATE stores SET setup_finished_at = now()
WHERE EXISTS (SELECT 1 FROM markets) AND EXISTS (SELECT 1 FROM products);
