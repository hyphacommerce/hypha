-- Stripe's payments so far (design note 0030, final review): their reference is the
-- PaymentIntent they collect through, so another intent's money for one already paid is
-- refused (and given back) from now. One whose reference a start racing its payment
-- replaced (rare, and no longer possible) takes that intent; staff fix it at Stripe.
UPDATE payments SET attempt = reference
    WHERE provider = 'spore_stripe'
      AND attempt IS NULL
      AND reference LIKE 'pi\_%';
