"use server";

// Every change a shopper makes, as server actions: the cart ID stays in an httpOnly
// cookie and the publishable key on the server. Pages other than search, cart,
// checkout and account are static (ISR), so they never read the cookie: the header and
// the cart drawer read the cart through `loadCart` once the page is in the browser,
// and the cart's actions answer with the cart, for the drawer to show. The forms take
// typed input and get the shopper's message back when the API refused.

import { refresh } from "next/cache";
import { cookies } from "next/headers";
import { redirect } from "next/navigation";

import type { StorefrontSchemas } from "@hyphacommerce/sdk/storefront";

import { api, describe, loadCountries, shopperApi } from "@/lib/api";
import { type ListingPage, readListingPage } from "@/lib/listing-data";
import { formatRange } from "@/lib/money";
import { COUNTRY_COOKIE } from "@/lib/routing";
import { paymentElementModule } from "@/lib/app-elements";
import { checkOutCart } from "@/lib/checkout";
import { accountApi, forgetSession, sessionHeaders } from "@/lib/account";
import { cartId, forgetCart, link, mode, rememberCart } from "@/lib/site";

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

/** A new cart in `country`, remembered in the shopper's cookies; a signed-in shopper's
 * is theirs from the start. */
async function newCart(country: string): Promise<string> {
  const session = await sessionHeaders();
  let answer = await (await shopperApi(session)).POST("/storefront/carts", { body: { country } });
  if (answer.error?.code === "invalid_token" && "authorization" in session) {
    // The session ended meanwhile (signed out everywhere, expired): the cookie goes,
    // and the shopper carries on as a guest.
    await forgetSession();
    answer = await (await shopperApi()).POST("/storefront/carts", { body: { country } });
  }
  if (answer.error) throw new Error(`couldn't create a cart: ${answer.error.code}`);
  await rememberCart(country, answer.data.id);
  return answer.data.id;
}

/** Whether a refusal means the cart can't take changes any more: gone (or another
 * channel's), or checked out. A new cart takes the line instead. */
function cartClosed(error: ApiError): boolean {
  return error.code === "cart_not_found" || error.code === "cart_completed";
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
  const { data, response } = await api().GET("/storefront/carts/{id}", {
    params: { path: { id } },
  });
  if (data && !data.order) return data;
  // Gone or an order now: forget it (and its flag), so pages stop asking.
  if (data || response.status === 404) await forgetCart(country);
  return null;
}

/** A variant's prices: what the product page shows and follows the quantity with. */
export type VariantPrices = Pick<
  StorefrontSchemas["StorefrontVariantResponse"],
  "price" | "compare_at_price" | "on_sale" | "price_breaks"
>;

/**
 * The signed-in shopper's own prices for a product page's variants, by variant ID: the
 * product read with their session, so price lists for their customer groups count. The
 * page is static and shows everyone's prices; it asks this once, only when the browser
 * says there's a session. `null` without one (or when it ended: the cookie goes), or
 * when the product can't be read.
 */
export async function customerPrices(input: {
  country: string;
  handle: string;
}): Promise<Record<string, VariantPrices> | null> {
  const client = await accountApi();
  if (!client) return null;
  const { data, response } = await client.GET("/storefront/products/by-handle/{handle}", {
    params: { path: { handle: input.handle }, query: { country: input.country } },
  });
  if (!data) {
    if (response.status === 401) await forgetSession();
    return null;
  }
  return Object.fromEntries(
    data.variants.map((v) => [
      v.id,
      {
        price: v.price,
        compare_at_price: v.compare_at_price,
        on_sale: v.on_sale,
        price_breaks: v.price_breaks,
      },
    ]),
  );
}

/** Adds a variant to the cart (opening one if needed), and answers with the cart. */
export async function addToCart(input: {
  country: string;
  variant: string;
  quantity: number;
}): Promise<CartResult> {
  const body = { variant: input.variant, quantity: Math.max(1, Math.floor(input.quantity) || 1) };
  const add = (id: string) =>
    api().POST("/storefront/carts/{id}/lines", { params: { path: { id } }, body });
  // Straight to the cart in the cookie; only a cart that's gone or checked out makes a
  // new one (no read first: one call for the usual case).
  const existing = await cartId(input.country);
  let result = existing ? await add(existing) : undefined;
  if (!result || (result.error && cartClosed(result.error))) {
    result = await add(await newCart(input.country));
  }
  const { data, error } = result;
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

/** Checkout's first step: the email, the marketing box and the shipping address,
 * together. One after the other: all write the cart, so at once they'd race for its
 * version. */
export async function saveDetails(
  country: string,
  details: Details,
  marketingOptIn: boolean,
): Promise<ActionResult> {
  const id = await currentCart(country);
  const client = api();
  const email = await client.PUT("/storefront/carts/{id}/email", {
    params: { path: { id } },
    body: { email: details.email.trim() },
  });
  if (email.error) return refused(email.error);
  // "Email me with news and offers": checkout records the consent when it's
  // ticked; unticked never unsubscribes anyone.
  if (email.data.marketing_opt_in !== marketingOptIn) {
    const marketing = await client.PUT("/storefront/carts/{id}/marketing", {
      params: { path: { id } },
      body: { opt_in: marketingOptIn },
    });
    if (marketing.error) return refused(marketing.error);
  }
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
  // The page re-renders in this action's answer: no second request to refresh it.
  refresh();
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
  refresh();
}

/** Checkout's third step, when there's something to pay: how the shopper pays. */
export async function selectPaymentMethod(input: {
  country: string;
  method: string;
}): Promise<ActionResult> {
  const id = await currentCart(input.country);
  const { error } = await api().PUT("/storefront/carts/{id}/payment-method", {
    params: { path: { id } },
    body: { payment_method: input.method },
  });
  if (error) return refused(error);
  refresh();
}

/** What a refused "Place order" tells the form: the step to go back to, or that the
 * payment isn't confirmed yet (`pending`: ask again in a moment). */
export type PlaceOrderResult = { error: string; step?: "payment"; pending?: boolean } | undefined;

/**
 * Places the order at the total the shopper saw, with the payment method they chose: a
 * total that moved since is refused (`total_changed`) rather than charged. The
 * idempotency key (one per render of the checkout) makes a double click place one order.
 * A method paid online needs its money committed first (the card form on the checkout,
 * `startCartPayment`); until the provider says so the answer is `pending`. A cart
 * checked out already (the provider's webhook got there first) is answered with its
 * order.
 */
export async function placeOrder(input: {
  country: string;
  /** Absent when coming back from a payment provider's page: the payment's own total. */
  total?: { amount: number; currency: string };
  attempt: string;
}): Promise<PlaceOrderResult> {
  await currentCart(input.country);
  const result = await checkOutCart(input);
  if ("error" in result) {
    if (result.code === "payment_method_required" || result.code === "payment_method_unavailable") {
      // Back to the payment step, re-rendered with the methods offered now.
      refresh();
      return { error: result.error, step: "payment" };
    }
    if (result.code === "payment_pending") return { error: result.error, pending: true };
    if (result.code === "payment_failed" || result.code === "payment_required") {
      return { error: result.error, step: "payment" };
    }
    return { error: result.error };
  }
  // Instructions (cash on delivery, bank transfer) are on the confirmation, read from
  // the order's payment; an online payment was made before the order.
  redirect(link(input.country, `/order/${result.order}/confirmed`));
}

/** What starting the cart's payment answered. */
export type CartPaymentResult = {
  nextAction: StorefrontSchemas["NextActionResponse"] | null;
  /** The shopper's message when it couldn't start. */
  error: string | null;
  /** The app element's module, for a `client_widget` (from the installed apps). */
  module: string | null;
  /** The cart's money is committed already (a reload after paying): place the order. */
  committed: boolean;
};

/**
 * Starts paying for the cart with its online method, before the order exists: the
 * provider's card form (mounted on the checkout) or its page. Started again (the page
 * reloaded), the provider resumes the same payment. `returnUrl` is where a provider's
 * page sends the shopper back (`/checkout/complete`), made absolute in the browser.
 */
export async function startCartPayment(input: {
  country: string;
  total: { amount: number; currency: string };
  returnUrl: string;
}): Promise<CartPaymentResult> {
  const id = await currentCart(input.country);
  const returnUrl = /^https?:\/\//.test(input.returnUrl) ? input.returnUrl : undefined;
  const { data, error } = await api().POST("/storefront/carts/{id}/payment", {
    params: { path: { id } },
    body: { expected_total: input.total, return_url: returnUrl },
  });
  const none = { nextAction: null, module: null, committed: false };
  if (error) {
    if (error.code === "cart_payment_committed") return { ...none, error: null, committed: true };
    // The page re-renders with what changed (the total, the methods).
    if (error.code === "total_changed" || error.code === "payment_method_unavailable") refresh();
    return { ...none, error: describe(error) };
  }
  const next = data.next_action ?? null;
  return {
    nextAction: next,
    error: data.payment_error ? describe(data.payment_error) : null,
    module: next?.type === "client_widget" ? await paymentElementModule(next.element) : null,
    committed: false,
  };
}

/**
 * Switches country (the picker): remembers it, and goes to the same page there. With
 * folders, that's the page under the new country's folder.
 */
export async function setCountry(input: { country: string; path: string }) {
  const country = input.country.toUpperCase();
  // A path on this site only: `//host` or `/\host` would leave it, and so would a
  // "country" that isn't one (it becomes the folder).
  const path = /^\/(?![/\\])/.test(input.path) ? input.path : "/";
  if (!/^[A-Z]{2}$/.test(country)) redirect(path);
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

/** The header's suggestions: the first five search results for `q`. */
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
 * or paged in the browser: the page itself can't read the URL's query without becoming
 * dynamic.
 */
export async function listingPage(input: {
  country: string;
  category?: string;
  /** Only what's on sale (the sale page). */
  onSale?: boolean;
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
    onSale: input.onSale,
  });
}
