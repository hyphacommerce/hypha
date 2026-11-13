// Server-only: checking out the shopper's cart, shared by the "Place order" action and
// the route a payment provider's page sends the shopper back to.

import { api, describe } from "./api";
import { actionT } from "./i18n";
import type { Locale } from "./routing";
import { cartId, forgetCart, rememberOrder } from "./site";

/** What checking out came to: the order placed, or the API's refusal with the shopper's
 * message for it. */
export type CheckedOut = { order: string } | { code: string; error: string };

/**
 * Checks out the shopper's cart in `place` (the page's: its answer is in the page's
 * language), at `total` when the shopper confirmed one
 * (absent: back from a provider's page, the payment's own total). Keeps the order's
 * access token for this buyer and forgets the cart. A cart checked out already (the
 * provider's webhook got there first) answers with its order.
 */
export async function checkOutCart(input: {
  place: Locale;
  total?: { amount: number; currency: string };
  attempt: string;
}): Promise<CheckedOut> {
  const t = await actionT(input.place.segment);
  const id = await cartId(input.place.country);
  if (!id) return { code: "cart_not_found", error: t("errors.cart_not_found") };
  const { data, error } = await api().POST("/storefront/carts/{id}/checkout", {
    params: { path: { id }, header: { "Idempotency-Key": input.attempt } },
    body: input.total ? { expected_total: input.total } : {},
  });
  if (error) return { code: error.code, error: describe(t, error) };
  // The confirmation reads the order with its access token, kept here for this buyer:
  // the order's ID alone reads nothing.
  await rememberOrder(data.id, data.access_token);
  await forgetCart(input.place.country);
  return { order: data.id };
}

/**
 * Whether the shopper's cart in `country` pays online: a method whose provider isn't
 * `manual`. Only such a cart is checked out by coming back from a provider's page; a
 * manual one is placed with "Place order".
 */
export async function paysOnline(country: string): Promise<boolean> {
  const id = await cartId(country);
  if (!id) return false;
  const client = api();
  const [{ data: cart }, { data: methods }] = await Promise.all([
    client.GET("/storefront/carts/{id}", { params: { path: { id } } }),
    client.GET("/storefront/carts/{id}/payment-methods", { params: { path: { id } } }),
  ]);
  const method = methods?.items.find((m) => m.id === cart?.payment_method);
  return method !== undefined && method.provider !== "manual";
}
