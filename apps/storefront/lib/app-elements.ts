// Server-only: the storefront elements installed apps offer (docs/apps/elements.md), by
// target. Cached like the store's details (no cookies, so static pages may read it); an
// API that can't answer means no elements, never a broken page.

import type { StorefrontSchemas } from "@hypha/sdk/storefront";
import { unstable_cache } from "next/cache";
import { cache } from "react";

import { api } from "./api";
import { FRESH_SECONDS, TAGS } from "./cache-tags";

export type AppElement = StorefrontSchemas["ElementExtension"];

export type StorefrontTarget =
  | "checkout.payment"
  | "product.block"
  | "cart.block"
  | "order.confirmation";

/** Every installed app's storefront elements, once per request and cached. */
export const loadElements = cache(
  unstable_cache(
    async (): Promise<AppElement[]> => {
      try {
        const { data } = await api().GET("/storefront/app-extensions");
        return data?.elements ?? [];
      } catch {
        return [];
      }
    },
    ["app-elements"],
    { tags: [TAGS.store], revalidate: FRESH_SECONDS },
  ),
);

/** The elements for one target. */
export async function elementsFor(target: StorefrontTarget): Promise<AppElement[]> {
  return (await loadElements()).filter((e) => e.target === target);
}

/**
 * The module for a payment's `client_widget` element (its tag): the installed app
 * element on `checkout.payment` with that tag, or none.
 */
export async function paymentElementModule(tag: string): Promise<string | null> {
  const found = (await elementsFor("checkout.payment")).find((e) => e.tag === tag);
  return found?.module ?? null;
}
