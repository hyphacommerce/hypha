-- The provider's attempt a payment collects through (design note 0030, final review: a
-- Stripe PaymentIntent), set only by a provider's report naming it, apart from the
-- reference (which staff's text or an operation's own ID may replace). Payments before
-- this have none: their next report naming an attempt sets it.
ALTER TABLE payments
    ADD COLUMN attempt text;
