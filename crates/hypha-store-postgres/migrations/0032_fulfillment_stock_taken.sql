-- Whether a fulfilment line's units came off stock (design note 0018, review fix): taking
-- the fulfilment back restocks only those. Rows from before this migration are dev
-- data; they count as taken, as `fulfil` did for tracked items.
ALTER TABLE fulfillment_lines ADD COLUMN stock_taken boolean NOT NULL DEFAULT true;
ALTER TABLE fulfillment_lines ALTER COLUMN stock_taken DROP DEFAULT;
