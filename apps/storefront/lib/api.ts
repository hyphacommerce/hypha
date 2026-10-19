// Server-only: how the storefront talks to the Hypha storefront API (design note 0023,
// SF2). The publishable key stays here; pages render on the server and changes go
// through server actions, so the browser never needs it. Never import this from a
// "use client" file.

import { apiFetch } from "#api-fetch";
import { type StorefrontSchemas, createStorefrontClient } from "@hypha/sdk/storefront";
import { unstable_cache } from "next/cache";
import { headers } from "next/headers";
import { notFound } from "next/navigation";
import { cache } from "react";

import { FRESH_SECONDS, TAGS } from "./cache-tags";

export type Country = StorefrontSchemas["StorefrontCountryResponse"];

function env(name: string): string {
  const value = process.env[name];
  if (!value) throw new Error(`${name} is not set (see apps/storefront/.env.example)`);
  return value;
}

/** A client with the storefront's publishable key (through the API's Service Binding
 * on Workers). */
export function api(extra: Record<string, string> = {}) {
  return createStorefrontClient(env("HYPHA_API_URL"), env("STOREFRONT_KEY"), {
    fetch: apiFetch(),
    headers: extra,
  });
}

/**
 * A client that passes the shopper's address on, for the calls Hypha limits per address
 * (creating a cart; design note 0009), so the limit counts the shopper, not this server.
 * Only from a source the browser can't forge, as the admin's (`apps/admin/lib/hypha.ts`):
 * Cloudflare's `cf-connecting-ip` on Workers (the API believes it through its own Service
 * Binding, design note 0026), or, behind this deployment's own proxy
 * (`STOREFRONT_TRUST_X_FORWARDED_FOR=true`, a VPS's Caddy), the last `x-forwarded-for`
 * entry, which the API believes from its trusted proxies. Server actions only: it reads
 * the request's headers.
 */
export async function shopperApi() {
  const h = await headers();
  let ip = h.get("cf-connecting-ip")?.trim();
  if (!ip && process.env.STOREFRONT_TRUST_X_FORWARDED_FOR === "true") {
    ip = h.get("x-forwarded-for")?.split(",").at(-1)?.trim();
  }
  return api(ip ? { "x-forwarded-for": ip } : {});
}

/**
 * The data of a read a page needs. A missing resource goes to the 404 page; anything
 * else is a bug or an outage, so it throws to the error page.
 */
export function loaded<T>(result: { data?: T; response: Response }): T {
  if (result.data !== undefined) return result.data;
  if (result.response.status === 404) notFound();
  throw new Error(`${result.response.url} failed with ${result.response.status}`);
}

/** The store's public details, once per request (and cached, tagged `store`). */
export const loadStore = cache(
  unstable_cache(async () => loaded(await api().GET("/storefront/store")), ["store"], {
    tags: [TAGS.store],
    revalidate: FRESH_SECONDS,
  }),
);

/** Every country a market sells to, with its currency and minor units (SF6). */
export const loadCountries = cache(
  unstable_cache(
    async (): Promise<Country[]> => loaded(await api().GET("/storefront/countries")).items,
    ["countries"],
    { tags: [TAGS.store, TAGS.prices], revalidate: FRESH_SECONDS },
  ),
);

/** The country of a URL folder (`hr`), or 404 when no market sells there. */
export async function countryOf(folder: string): Promise<Country> {
  const code = folder.toUpperCase();
  const country = (await loadCountries()).find((c) => c.country === code);
  if (!country) notFound();
  return country;
}

/** The shown category tree, once per request (and cached, tagged `categories`). */
export const loadCategories = cache(
  unstable_cache(
    async () => loaded(await api().GET("/storefront/categories")).items,
    ["categories"],
    {
      tags: [TAGS.categories],
      revalidate: FRESH_SECONDS,
    },
  ),
);

/**
 * A category page's category, cached and tagged with its handle, once per request (its
 * metadata and the page share it). A missing or hidden one is `null` (the page 404s),
 * not cached as an error.
 */
export const loadCategory = cache((handle: string) =>
  unstable_cache(
    async () => {
      const { data } = await api().GET("/storefront/categories/by-handle/{handle}", {
        params: { path: { handle } },
      });
      return data ?? null;
    },
    ["category", handle],
    { tags: [TAGS.categories, TAGS.category(handle)], revalidate: FRESH_SECONDS },
  )(),
);

/**
 * A product page's product, priced and stocked in `country`, cached and tagged with its
 * handle (the webhook expires it), once per request (its metadata and the page share
 * it). A missing one is `null`.
 */
export const loadProduct = cache((handle: string, country: string) =>
  unstable_cache(
    async () => {
      const { data } = await api().GET("/storefront/products/by-handle/{handle}", {
        params: { path: { handle }, query: { country } },
      });
      return data ?? null;
    },
    ["product", handle, country],
    { tags: [TAGS.products, TAGS.product(handle), TAGS.prices], revalidate: FRESH_SECONDS },
  )(),
);

/** A message for the shopper, by the API's stable error code. */
export function describe(error: { code: string; error: string }): string {
  switch (error.code) {
    case "out_of_stock":
      return "There isn't enough of that in stock.";
    case "product_not_published":
    case "product_not_active":
    case "product_not_found":
      return "That product isn't for sale here any more.";
    case "invalid_email":
      return "Enter an email address, like ana@example.com.";
    case "invalid_address":
    case "field_required":
      return "Fill in your name, street address and city.";
    case "no_shipping_to_country":
      return "We don't ship to this country yet.";
    case "total_changed":
      return "Prices or shipping changed since you looked. Check the total and place the order again.";
    case "cart_completed":
      return "This cart was already checked out.";
    case "cart_empty":
      return "Your cart is empty.";
    case "cart_unavailable":
      return "Something in your cart can't be bought right now. Remove it to continue.";
    case "email_required":
    case "cart_email_missing":
      return "Enter your email address.";
    case "shipping_address_required":
      return "Enter where your order ships.";
    case "shipping_option_required":
    case "shipping_option_unavailable":
      return "Choose how your order ships.";
    case "address_country_mismatch":
      return "The address must be in the country you're shopping in.";
    case "payment_method_required":
      return "Choose how you'll pay.";
    case "payment_method_unavailable":
    case "payment_method_not_found":
      return "That payment method can't be used for this order now. Choose another.";
    case "app_unavailable":
      return "The payment service didn't answer. Try again in a moment.";
    case "order_not_payable":
      return "There's nothing left to pay for this order.";
    case "order_cancelled":
      return "This order was cancelled.";
    case "order_not_found":
      return "We can't find this order. Open it from its email.";
    default:
      return error.error;
  }
}
