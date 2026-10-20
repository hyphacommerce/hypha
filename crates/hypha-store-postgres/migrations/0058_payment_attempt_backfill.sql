-- Stripe's open payments so far (design note 0030, final review): their reference is
-- the PaymentIntent their start made, so it's the attempt they collect through. Not paid
-- ones: a start racing their payment may have replaced the reference with an intent that
-- collected nothing; their next report naming their own reference keeps it.
UPDATE payments SET attempt = reference
    WHERE provider = 'spore_stripe'
      AND status IN ('pending', 'authorized')
      AND attempt IS NULL
      AND reference LIKE 'pi\_%';
