// Server-only: how the storefront talks to the Hypha storefront API (design note 0023,
// SF2). The publishable key stays here; pages render on the server and changes go
// through server actions, so the browser never needs it. Never import this from a
// "use client" file.

import { type StorefrontSchemas, createStorefrontClient } from "@hypha/sdk/storefront";
import { notFound } from "next/navigation";
import { cache } from "react";

export type Country = StorefrontSchemas["StorefrontCountryResponse"];

function env(name: string): string {
  const value = process.env[name];
  if (!value) throw new Error(`${name} is not set (see apps/storefront/.env.example)`);
  return value;
}

/** A client with the storefront's publishable key. */
export function api() {
  return createStorefrontClient(env("HYPHA_API_URL"), env("STOREFRONT_KEY"));
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

/** The store's public details, once per request. */
export const loadStore = cache(async () => loaded(await api().GET("/storefront/store")));

/** Every country a market sells to, with its currency and minor units (SF6). */
export const loadCountries = cache(async (): Promise<Country[]> => {
  return loaded(await api().GET("/storefront/countries")).items;
});

/** The country of a URL folder (`hr`), or 404 when no market sells there. */
export async function countryOf(folder: string): Promise<Country> {
  const code = folder.toUpperCase();
  const country = (await loadCountries()).find((c) => c.country === code);
  if (!country) notFound();
  return country;
}

/** The shown category tree, once per request. */
export const loadCategories = cache(
  async () => loaded(await api().GET("/storefront/categories")).items,
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
    default:
      return error.error;
  }
}
