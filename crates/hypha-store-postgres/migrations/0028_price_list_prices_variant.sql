-- Fixed prices by variant (`PriceListRepo::lists_pricing`): cleaning up after a removed
-- variant (design note 0016, section 6) looks them up this way. The primary key starts
-- with the price list, so it can't serve this.
CREATE INDEX price_list_prices_variant ON price_list_prices (variant_id);
