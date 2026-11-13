// Server-only: reading the shopper's cart for a page, in the page's language.

import type { StorefrontSchemas } from "@hyphacommerce/sdk/storefront";

import { api } from "./api";
import { cartLanguageFor } from "./cart-language";
import type { Locale } from "./routing";

/** A cart read: the cart (absent when the API refused), the read's status, and whether
 * the read moved the cart to the page's language. */
export type CartRead = {
  cart?: StorefrontSchemas["CartResponse"];
  status: number;
  moved: boolean;
};

/**
 * The cart `id` for a page in `place`. The page's language wins: a cart that reads in
 * another (a link shared from another language, a second tab) is moved to the page's
 * (`PUT …/language`, which answers the cart in it), so its lines, messages and the
 * order's emails follow what the shopper reads. Once moved, the next read matches, so
 * it's written once per change of language. A checked-out cart is left as it is.
 */
export async function readCart(id: string, place: Locale): Promise<CartRead> {
  const client = api();
  const { data, response } = await client.GET("/storefront/carts/{id}", {
    params: { path: { id } },
  });
  const read = { cart: data, status: response.status, moved: false };
  if (!data || data.order) return read;
  const language = cartLanguageFor(data, response.headers.get("content-language"), place.language);
  if (language === undefined) return read;
  const moved = await client.PUT("/storefront/carts/{id}/language", {
    params: { path: { id } },
    body: { language },
  });
  // Refused (the cart was checked out meanwhile): the read stands.
  return moved.data ? { cart: moved.data, status: moved.response.status, moved: true } : read;
}
