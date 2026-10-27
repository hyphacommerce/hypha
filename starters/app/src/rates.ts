// The `shipping.rates` extension point: Hypha asks for a quote while a shopper checks out,
// for each shipping option whose rate names one of this app's services (see the README),
// and shows the answer beside the zone's other offers. This one quotes a flat rate in the
// cart's currency; a real carrier integration would call the carrier here (within the
// point's timeout, 3 seconds by default).

import { type ShippingRatesResponse, defineExtensionHandler } from "@hyphacommerce/app";

/** Minor units, as everywhere in Hypha's API: 990 EUR cents. */
const FLAT_RATE = 990;

export const rates = defineExtensionHandler("shipping.rates", (body): ShippingRatesResponse => {
  // `body.request` is the shipping group: its currency, destination, items and subtotal.
  const { currency } = body.request;
  return {
    rates: [
      {
        service: "starter_flat",
        name: "Flat rate",
        amount: { amount: FLAT_RATE, currency },
        estimate: { min_days: 2, max_days: 5 },
      },
    ],
  };
});
