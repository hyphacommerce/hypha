-- A customer's orders (design note 0032, §12 step 1): the orders list's `customer`
-- filter and the statistics on the customer's page (count, first and last order, amount
-- spent per currency) read a customer's orders by this index, newest first.
CREATE INDEX orders_by_customer ON orders (customer_id, id);
