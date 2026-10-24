-- Claims don't move the counter of a code without a limit of its own: its uses are
-- counted from the usage rows, by discount and code.
CREATE INDEX discount_usages_by_code ON discount_usages (discount_id, code)
    WHERE code IS NOT NULL;
