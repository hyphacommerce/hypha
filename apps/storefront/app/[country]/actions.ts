"use server";

// Every change a shopper makes, as server actions (design note 0023, SF5): the cart ID
// stays in an httpOnly cookie and the publishable key on the server. Pages other than
// search, cart, checkout and account are static (ISR), so they never read the cookie:
// the header and the cart drawer read the cart through `loadCart` once the page is in
// the browser, and the cart's actions answer with the cart, for the drawer to show.
// The forms take typed input and get the shopper's message back when the API refused.

import { cookies } from "next/headers";
import { redirect } from "next/navigation";

import type { StorefrontSchemas } from "@hypha/sdk/storefront";

import { api, describe, loadCountries } from "@/lib/api";
import { type ListingPage, readListingPage } from "@/lib/listing-data";
import { formatRange } from "@/lib/money";
import { COUNTRY_COOKIE } from "@/lib/routing";
import { cartId, forgetCart, link, mode, orderTokenCookie, rememberCart } from "@/lib/site";

/** What a refused action tells its form. */
export type ActionResult = { error: string } | undefined;

/** The shopper's cart as the API prices it. */
export type Cart = StorefrontSchemas["CartResponse"];

/** A cart action's answer: the cart as it is now, or why it was refused. */
export type CartResult = { cart: Cart | null; error?: string };

type ApiError = { code: string; error: string };

function refused(error: ApiError): ActionResult {
  return { error: describe(error) };
}

/** `value` trimmed, or `null` for a blank optional field. */
function optional(value: string | undefined): string | null {
  const trimmed = (value ?? "").trim();
  return trimmed === "" ? null : trimmed;
}

/** The shopper's open cart in `country`: the one in the cookie, or a new one. */
async function openCart(country: string): Promise<string> {
  const client = api();
  const existing = await cartId(country);
  if (existing) {
    const { data } = await client.GET("/storefront/carts/{id}", {
      params: { path: { id: existing } },
    });
    if (data && !data.order) return existing;
  }
  const { data, error } = await client.POST("/storefront/carts", { body: { country } });
  if (error) throw new Error(`couldn't create a cart: ${error.code}`);
  await rememberCart(country, data.id);
  return data.id;
}

/** The cart in the cookie, or back to the (empty) cart page without one. */
async function currentCart(country: string): Promise<string> {
  const id = await cartId(country);
  if (!id) redirect(link(country, "/cart"));
  return id;
}

/** The shopper's open cart in `country`, priced; `null` without one (or once it's an
 * order). For the header and the drawer, which can't read it while the page renders. */
export async function loadCart(country: string): Promise<Cart | null> {
  const id = await cartId(country);
  if (!id) return null;
  const { data } = await api().GET("/storefront/carts/{id}", { params: { path: { id } } });
  return data && !data.order ? data : null;
}

/** Adds a variant to the cart (opening one if needed), and answers with the cart. */
export async function addToCart(input: {
  country: string;
  variant: string;
  quantity: number;
}): Promise<CartResult> {
  const id = await openCart(input.country);
  const { data, error } = await api().POST("/storefront/carts/{id}/lines", {
    params: { path: { id } },
    body: { variant: input.variant, quantity: Math.max(1, Math.floor(input.quantity) || 1) },
  });
  if (error) return { cart: await loadCart(input.country), error: describe(error) };
  return { cart: data };
}

/** Sets a line's quantity (0 removes it), and answers with the cart. */
export async function setQuantity(input: {
  country: string;
  variant: string;
  quantity: number;
}): Promise<CartResult> {
  const id = await cartId(input.country);
  if (!id) return { cart: null };
  const client = api();
  const path = { id, variant: input.variant };
  const { data, error } =
    input.quantity > 0
      ? await client.PUT("/storefront/carts/{id}/lines/{variant}", {
          params: { path },
          body: { quantity: Math.floor(input.quantity) },
        })
      : await client.DELETE("/storefront/carts/{id}/lines/{variant}", { params: { path } });
  if (error) return { cart: await loadCart(input.country), error: describe(error) };
  return { cart: data ?? (await loadCart(input.country)) };
}

export type Details = {
  email: string;
  first_name: string;
  last_name: string;
  company: string;
  line1: string;
  line2: string;
  postal_code: string;
  city: string;
  province: string;
  phone: string;
};

/** Checkout's first step: the email and the shipping address, together. */
export async function saveDetails(country: string, details: Details): Promise<ActionResult> {
  const id = await currentCart(country);
  const client = api();
  const email = await client.PUT("/storefront/carts/{id}/email", {
    params: { path: { id } },
    body: { email: details.email.trim() },
  });
  if (email.error) return refused(email.error);
  const address = await client.PUT("/storefront/carts/{id}/shipping-address", {
    params: { path: { id } },
    body: {
      address: {
        first_name: optional(details.first_name),
        last_name: details.last_name.trim(),
        company: optional(details.company),
        line1: details.line1.trim(),
        line2: optional(details.line2),
        city: details.city.trim(),
        postal_code: optional(details.postal_code),
        province: optional(details.province),
        country,
        phone: optional(details.phone),
      },
    },
  });
  if (address.error) return refused(address.error);
}

/** Checkout's second step: a shipping option for one group of lines. */
export async function selectShipping(input: {
  country: string;
  group: string;
  option: string;
}): Promise<ActionResult> {
  const id = await currentCart(input.country);
  const { error } = await api().PUT("/storefront/carts/{id}/shipping/{group_id}", {
    params: { path: { id, group_id: input.group } },
    body: { option: input.option },
  });
  if (error) return refused(error);
}

/**
 * Places the order, unpaid (0021, SU8), at the total the shopper saw: a total that moved
 * since is refused (`total_changed`) rather than charged. The idempotency key (one per
 * render of the checkout) makes a double click place one order.
 */
export async function placeOrder(input: {
  country: string;
  total: { amount: number; currency: string };
  attempt: string;
}): Promise<ActionResult> {
  const id = await currentCart(input.country);
  const { data, error } = await api().POST("/storefront/carts/{id}/checkout", {
    params: { path: { id }, header: { "Idempotency-Key": input.attempt } },
    body: { expected_total: input.total },
  });
  if (error) return refused(error);
  // The confirmation reads the order with its access token, kept here for this buyer:
  // the order's ID alone reads nothing (design note 0024, §7).
  (await cookies()).set(orderTokenCookie(data.id), data.access_token, {
    httpOnly: true,
    sameSite: "lax",
    secure: process.env.NODE_ENV === "production",
    path: "/",
    maxAge: 60 * 60 * 24 * 30,
  });
  await forgetCart(input.country);
  redirect(link(input.country, `/order/${data.id}/confirmed`));
}

/**
 * Switches country (the picker): remembers it, and goes to the same page there. With
 * folders, that's the page under the new country's folder.
 */
export async function setCountry(input: { country: string; path: string }) {
  const country = input.country.toUpperCase();
  const path = input.path || "/";
  (await cookies()).set(COUNTRY_COOKIE, country, {
    sameSite: "lax",
    path: "/",
    maxAge: 60 * 60 * 24 * 365,
  });
  if (mode() === "single") redirect(path);
  // `/hr/p/mug` → `/p/mug`, then into the new folder.
  const rest = path.replace(/^\/[a-z]{2}(?=\/|$)/, "") || "/";
  redirect(link(country, rest));
}

/** A search suggestion: a product to go to, priced in the shopper's country. */
export type Suggestion = {
  id: string;
  title: string;
  price: string;
  href: string;
  image: string | null;
};

/** The header's suggestions: the first five search results for `q` (0024, §7). */
export async function suggest(country: string, q: string): Promise<Suggestion[]> {
  const text = q.trim();
  if (text === "") return [];
  const known = (await loadCountries()).find((c) => c.country === country);
  if (!known) return [];
  const { data } = await api().GET("/storefront/search", {
    params: { query: { country, q: text, sort: "relevance", limit: 5 } },
  });
  return (data?.items ?? []).map((hit) => ({
    id: hit.listing,
    title: hit.title,
    price: formatRange(hit.listing_price.min, hit.listing_price.max, known.minor_units),
    href: link(country, `/p/${hit.handle}`),
    image: hit.image?.kind === "image" ? (hit.image.thumbnail_url ?? hit.image.url) : null,
  }));
}

/**
 * A listing's page for a query string, when a static category page is filtered, sorted
 * or paged in the browser (design note 0024, §7): the page itself can't read the URL's
 * query without becoming dynamic.
 */
export async function listingPage(input: {
  country: string;
  category?: string;
  query: string;
}): Promise<ListingPage> {
  const known = (await loadCountries()).find((c) => c.country === input.country);
  const params: Record<string, string[]> = {};
  for (const [key, value] of new URLSearchParams(input.query)) (params[key] ??= []).push(value);
  return readListingPage({
    country: input.country,
    digits: known?.minor_units ?? 2,
    params,
    category: input.category,
  });
}
