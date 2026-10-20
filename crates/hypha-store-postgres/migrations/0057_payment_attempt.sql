-- The provider's attempt a payment collects through (design note 0030, final review: a
-- Stripe PaymentIntent), set by its start or a provider's report naming it, apart from
-- the reference (which staff's text or an operation's own ID may replace).
ALTER TABLE payments
    ADD COLUMN attempt text;

-- Stripe's payments so far: their reference is their PaymentIntent unless staff or a
-- capture replaced it, so those that still name one are checked from now.
UPDATE payments SET attempt = reference
    WHERE provider = 'spore_stripe' AND reference LIKE 'pi\_%';
