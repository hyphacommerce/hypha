// Serbian in Latin script: `messages/sr-Latn/<namespace>.json`, loaded as one module
// when a request asks for it (`lib/messages.ts`).

import { mergeCatalogs } from "@hyphacommerce/i18n";

import account from "./sr-Latn/account.json" with { type: "json" };
import address from "./sr-Latn/address.json" with { type: "json" };
import cart from "./sr-Latn/cart.json" with { type: "json" };
import checkout from "./sr-Latn/checkout.json" with { type: "json" };
import common from "./sr-Latn/common.json" with { type: "json" };
import discounts from "./sr-Latn/discounts.json" with { type: "json" };
import errors from "./sr-Latn/errors.json" with { type: "json" };
import footer from "./sr-Latn/footer.json" with { type: "json" };
import header from "./sr-Latn/header.json" with { type: "json" };
import home from "./sr-Latn/home.json" with { type: "json" };
import listing from "./sr-Latn/listing.json" with { type: "json" };
import order from "./sr-Latn/order.json" with { type: "json" };
import product from "./sr-Latn/product.json" with { type: "json" };
import sale from "./sr-Latn/sale.json" with { type: "json" };
import search from "./sr-Latn/search.json" with { type: "json" };

export default mergeCatalogs({
  account,
  address,
  cart,
  checkout,
  common,
  discounts,
  errors,
  footer,
  header,
  home,
  listing,
  order,
  product,
  sale,
  search,
});
