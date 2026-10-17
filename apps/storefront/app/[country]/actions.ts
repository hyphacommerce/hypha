"use server";

// Every change a shopper makes, as server actions (design note 0023, SF5): the cart ID
// stays in an httpOnly cookie and the publishable key on the server. The forms are
// Astryx components (0024, AU12) holding their own state, so each action takes typed
// input and returns the shopper's message when the API refused, for the form to show.
// One that moves the shopper on (to the cart, the confirmation) redirects; one that
// changes the page it's on returns, and the form refreshes the page (and the header's
// cart count) with `router.refresh()`.

import { cookies } from "next/headers";
import { redirect } from "next/navigation";

import { api, describe, loadCountries } from "@/lib/api";
import { formatRange } from "@/lib/money";
import { COUNTRY_COOKIE } from "@/lib/routing";
import { cartId, forgetCart, link, mode, rememberCart } from "@/lib/site";

/** What a refused action tells its form. */
export type ActionResult = { error: string } | undefined;

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

/** Adds a variant to the cart, then shows the cart. */
export async function addToCart(input: {
  country: string;
  variant: string;
  quantity: number;
}): Promise<ActionResult> {
  const id = await openCart(input.country);
  const { error } = await api().POST("/storefront/carts/{id}/lines", {
    params: { path: { id } },
    body: { variant: input.variant, quantity: Math.max(1, Math.floor(input.quantity) || 1) },
  });
  if (error) return refused(error);
  redirect(link(input.country, "/cart"));
}

/** Sets a line's quantity; 0 removes it. */
export async function setQuantity(input: {
  country: string;
  variant: string;
  quantity: number;
}): Promise<ActionResult> {
  const id = await currentCart(input.country);
  const client = api();
  const path = { id, variant: input.variant };
  const { error } =
    input.quantity > 0
      ? await client.PUT("/storefront/carts/{id}/lines/{variant}", {
          params: { path },
          body: { quantity: Math.floor(input.quantity) },
        })
      : await client.DELETE("/storefront/carts/{id}/lines/{variant}", { params: { path } });
  if (error) return refused(error);
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
  const { error } = await api().POST("/storefront/carts/{id}/checkout", {
    params: { path: { id }, header: { "Idempotency-Key": input.attempt } },
    body: { expected_total: input.total },
  });
  if (error) return refused(error);
  // The confirmation reads the order through the checked-out cart.
  (await cookies()).set(`order_${input.country.toLowerCase()}`, id, {
    httpOnly: true,
    sameSite: "lax",
    secure: process.env.NODE_ENV === "production",
    path: "/",
    maxAge: 60 * 60 * 24,
  });
  await forgetCart(input.country);
  redirect(link(input.country, "/order"));
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
