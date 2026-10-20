-- Stripe's payments past pending from before attempts were kept (design note 0030, final
-- review): 0058 filled the open ones; their reference is the PaymentIntent they
-- collected through, so another intent's money for one of them is refused (and given
-- back) from now on, as for every newer payment. A payment whose reference a start racing
-- its payment replaced (a race this branch closed) takes that intent: staff reconcile
-- it in Stripe's dashboard if one is ever found.
UPDATE payments SET attempt = reference
    WHERE provider = 'spore_stripe'
      AND attempt IS NULL
      AND reference LIKE 'pi\_%';
