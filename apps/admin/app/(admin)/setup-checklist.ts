// The home page's setup checklist (design note 0021, SU9): worked out from data on
// every render, never stored. A read the user's scopes don't allow counts as not done,
// so the page still renders.

import { sessionClient } from "@/lib/hypha";

export type SetupStep = { label: string; description: string; href: string; isDone: boolean };

/** How many active products to look at for stock: each costs two reads. */
const STOCK_SAMPLE = 5;

export async function setupSteps(): Promise<SetupStep[]> {
  const client = await sessionClient();
  const [store, markets, locations, active, keys] = await Promise.all([
    client.GET("/admin/store"),
    client.GET("/admin/markets"),
    client.GET("/admin/locations"),
    client.GET("/admin/products", { params: { query: { status: "active", limit: 50 } } }),
    client.GET("/admin/api-keys"),
  ]);
  const products = active.data?.items ?? [];

  // Stock: a location exists and one of the first active products' first variant can
  // be sold (untracked, or with stock available somewhere).
  let hasStock = false;
  if ((locations.data?.items.length ?? 0) > 0) {
    const sample = await Promise.all(
      products.slice(0, STOCK_SAMPLE).map(async (p) => {
        const { data: product } = await client.GET("/admin/products/{id}", {
          params: { path: { id: p.id } },
        });
        const variant = product?.variants[0];
        if (!variant) return false;
        const { data: stock } = await client.GET("/admin/variants/{id}/inventory", {
          params: { path: { id: variant.id } },
        });
        if (!stock) return false;
        return !stock.tracked || stock.levels.some((level) => level.available > 0);
      }),
    );
    hasStock = sample.some(Boolean);
  }

  return [
    {
      label: "Add your store's details",
      description: "A contact email shoppers can write to, and your legal details.",
      href: "/settings/store",
      isDone: Boolean(store.data?.contact_email),
    },
    {
      label: "Create a market",
      description: "The countries you sell to and their currency.",
      href: "/markets",
      isDone: (markets.data?.items.length ?? 0) > 0,
    },
    {
      label: "Activate a product",
      description: "Drafts can't be sold.",
      href: "/products",
      isDone: products.length > 0,
    },
    {
      label: "Stock it",
      description: "Set stock at a location, or leave stock untracked.",
      href: "/products",
      isDone: hasStock,
    },
    {
      label: "Publish it to a channel",
      description: "A storefront sells what's published to its channel.",
      href: "/products",
      isDone: products.some((p) => p.channels.length > 0),
    },
    {
      label: "Create a storefront key",
      description: "The publishable key your storefront calls the API with.",
      href: "/settings/channels",
      isDone: (keys.data?.items ?? []).some((k) => k.kind === "publishable" && !k.revoked_at),
    },
  ];
}
