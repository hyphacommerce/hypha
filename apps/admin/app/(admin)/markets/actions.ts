"use server";

import { revalidatePath } from "next/cache";
import { redirect } from "next/navigation";

import { describe, sessionClient } from "@/lib/hypha";

/** The outcome of an edit: a message to show, if it failed. */
export type Result = { error?: string };

function refresh(market?: string) {
  revalidatePath("/markets");
  if (market) revalidatePath(`/markets/${market}`);
  // The home page's setup checklist counts markets too.
  revalidatePath("/");
}

/**
 * A market and its (empty) catalog, so it can sell as soon as products are added. A
 * catalog that fails leaves the market without one; its page offers to create it.
 */
export async function createMarket(input: {
  name: string;
  countries: string[];
  currency: string;
  pricesIncludeTax: boolean;
}): Promise<Result> {
  const client = await sessionClient();
  const { data: market, error } = await client.POST("/admin/markets", {
    body: {
      name: input.name,
      countries: input.countries,
      currency: input.currency,
      prices_include_tax: input.pricesIncludeTax,
    },
  });
  if (error) return { error: describe(error) };
  await client.POST("/admin/catalogs", { body: { market: market.id } });
  refresh();
  redirect(`/markets/${market.id}`);
}

export async function updateMarket(
  id: string,
  input: { name?: string; pricesIncludeTax?: boolean },
): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.PATCH("/admin/markets/{id}", {
    params: { path: { id } },
    body: { name: input.name, prices_include_tax: input.pricesIncludeTax },
  });
  if (error) return { error: describe(error) };
  refresh(id);
  return {};
}

/**
 * Makes the market's countries `countries`: adds the new ones first, then removes the
 * dropped ones, so a market is never left without one along the way.
 */
export async function setMarketCountries(
  id: string,
  current: string[],
  countries: string[],
): Promise<Result> {
  const client = await sessionClient();
  for (const country of countries.filter((c) => !current.includes(c))) {
    const { error } = await client.POST("/admin/markets/{id}/countries", {
      params: { path: { id } },
      body: { country },
    });
    if (error) {
      refresh(id);
      return { error: describe(error) };
    }
  }
  for (const country of current.filter((c) => !countries.includes(c))) {
    const { error } = await client.DELETE("/admin/markets/{id}/countries/{country}", {
      params: { path: { id, country } },
    });
    if (error) {
      refresh(id);
      return { error: describe(error) };
    }
  }
  refresh(id);
  return {};
}

export async function createCatalog(market: string): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.POST("/admin/catalogs", { body: { market } });
  if (error) return { error: describe(error) };
  refresh(market);
  return {};
}

export async function addCatalogProducts(
  market: string,
  catalog: string,
  products: string[],
): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.POST("/admin/catalogs/{id}/products", {
    params: { path: { id: catalog } },
    body: { products },
  });
  if (error) return { error: describe(error) };
  refresh(market);
  return {};
}

export async function removeCatalogProduct(
  market: string,
  catalog: string,
  product: string,
): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.DELETE("/admin/catalogs/{id}/products", {
    params: { path: { id: catalog } },
    body: { products: [product] },
  });
  if (error) return { error: describe(error) };
  refresh(market);
  return {};
}

/** Puts `list` on the market's catalog, beside the lists already there. */
export async function addCatalogPriceList(
  market: string,
  catalog: string,
  list: string,
): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.POST("/admin/catalogs/{id}/price-lists", {
    params: { path: { id: catalog } },
    body: { price_list: list },
  });
  if (error) return { error: describe(error) };
  refresh(market);
  revalidatePath("/markets/price-lists", "layout");
  return {};
}

/** Takes `list` off the market's catalog; the list itself stays. */
export async function removeCatalogPriceList(
  market: string,
  catalog: string,
  list: string,
): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.DELETE("/admin/catalogs/{id}/price-lists/{price_list}", {
    params: { path: { id: catalog, price_list: list } },
  });
  if (error) return { error: describe(error) };
  refresh(market);
  revalidatePath("/markets/price-lists", "layout");
  return {};
}
