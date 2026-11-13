"use server";

// The setup wizard's steps. Each saves through the same routes as the screen it stands
// in for, and returns instead of redirecting, so the wizard moves on to the next step.

import { revalidatePath } from "next/cache";

import { toHandle } from "@/lib/categories";
import { parseMoney } from "@/lib/format";
import { describe, loadCurrencies, sessionClient } from "@/lib/hypha";
import { getT } from "@/lib/i18n";

/** The outcome of a step: a message to show, if it failed. */
export type Result = { error?: string };
/** A result with a new publishable key's token, shown once. */
export type KeyResult = Result & { token?: string };

/** The wizard and the home page's checklist read the same data. */
function refresh() {
  revalidatePath("/", "layout");
}

/** Blank text is `null` (clears the field). */
function orNull(value: string): string | null {
  return value.trim() === "" ? null : value;
}

export async function saveStore(input: {
  name: string;
  contactEmail: string;
  timezone: string;
  language: string;
  /** Blank leaves the store's as it is: it can't change once a cost is recorded. */
  currency: string;
}): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.PATCH("/admin/store", {
    body: {
      name: input.name,
      contact_email: orNull(input.contactEmail),
      timezone: orNull(input.timezone),
      default_language: input.language,
      ...(input.currency ? { currency: input.currency } : {}),
    },
  });
  if (error) return { error: await describe(error) };
  refresh();
  return {};
}

/** A market and its (empty) catalog, as the Markets screen makes them. */
export async function addMarket(input: {
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
  if (error) return { error: await describe(error) };
  const { error: catalogError } = await client.POST("/admin/catalogs", {
    body: { market: market.id },
  });
  refresh();
  if (catalogError) {
    const t = await getT();
    return {
      error: t("setup.market_without_catalog", { error: await describe(catalogError) }),
    };
  }
  return {};
}

/**
 * A shipping zone with one flat-rate option. The zone is created first; if the option
 * then fails, the zone stays, and Settings → Shipping can add the option.
 */
export async function addShipping(input: {
  name: string;
  countries: string[];
  optionName: string;
  price: string;
  currency: string;
}): Promise<Result> {
  const t = await getT();
  const amount = parseMoney(input.price, input.currency, await loadCurrencies());
  if (amount === null) return { error: t("setup.shipping_rate_invalid") };
  const client = await sessionClient();
  const { data: zone, error } = await client.POST("/admin/shipping-zones", {
    body: { name: input.name, countries: input.countries },
  });
  if (error) return { error: await describe(error) };
  const { error: optionError } = await client.POST("/admin/shipping-zones/{id}/options", {
    params: { path: { id: zone.id } },
    body: {
      name: input.optionName,
      code: toHandle(input.optionName) || "standard",
      rate: {
        type: "builtin",
        basis: "flat",
        tables: [{ currency: input.currency, tiers: [{ amount, up_to: null }] }],
      },
      estimate: null,
      is_backup: false,
    },
  });
  refresh();
  if (optionError) {
    return { error: t("setup.shipping_zone_without_rate", { error: await describe(optionError) }) };
  }
  return {};
}

/**
 * A product ready to sell in `market`: created, added to the market's catalog, stocked
 * at `location` (when a count is given; otherwise left untracked, always sellable), and
 * activated. The location is made to serve the market, or its stock wouldn't count
 * there. Stops at the first failure and says what was done.
 */
export async function addProduct(input: {
  title: string;
  price: string;
  market: string;
  /** Whether to count its stock; untracked, it can always be sold. */
  track: boolean;
  stock: string;
  location: string | null;
}): Promise<Result> {
  const [client, t] = await Promise.all([sessionClient(), getT()]);
  const { data: market, error: marketError } = await client.GET("/admin/markets/{id}", {
    params: { path: { id: input.market } },
  });
  if (marketError) return { error: await describe(marketError) };
  const amount = parseMoney(input.price, market.currency, await loadCurrencies());
  if (amount === null) return { error: t("setup.product_price_invalid") };
  const count = input.track ? Number(input.stock.trim() || Number.NaN) : null;
  if (count !== null && (!Number.isInteger(count) || count < 0)) {
    return { error: t("setup.product_stock_invalid") };
  }

  const { data: product, error } = await client.POST("/admin/products", {
    body: {
      title: input.title,
      handle: toHandle(input.title),
      price: { amount, currency: market.currency },
    },
  });
  if (error) return { error: await describe(error) };
  // What a later step's failure leaves: the product as a draft, and why.
  const partly = async (
    what: "catalog" | "stock" | "status",
    e: Parameters<typeof describe>[0],
  ) => {
    refresh();
    return {
      error: t(`setup.product_partly_${what}`, {
        title: input.title,
        market: market.name,
        error: await describe(e),
      }),
    };
  };

  if (market.catalog) {
    const { error: e } = await client.POST("/admin/catalogs/{id}/products", {
      params: { path: { id: market.catalog } },
      body: { products: [product.id] },
    });
    if (e) return partly("catalog", e);
  }

  if (count !== null && input.location) {
    const { data: location, error: e } = await client.GET("/admin/locations/{id}", {
      params: { path: { id: input.location } },
    });
    if (e) return partly("stock", e);
    if (!location.markets.includes(market.id)) {
      const { error: e2 } = await client.PATCH("/admin/locations/{id}", {
        params: { path: { id: location.id } },
        body: { markets: [...location.markets, market.id] },
      });
      if (e2) return partly("stock", e2);
    }
    const variant = product.variants[0];
    const { data: item, error: e3 } = await client.POST("/admin/inventory-items", {
      body: { variant: variant.id, tracked: true },
    });
    if (e3) return partly("stock", e3);
    if (!item.id) return { error: t("setup.product_partly_stock_unknown", { title: input.title }) };
    const { error: e4 } = await client.PUT("/admin/inventory-items/{id}/levels/{location_id}", {
      params: { path: { id: item.id, location_id: location.id } },
      body: { on_hand: count },
    });
    if (e4) return partly("stock", e4);
  }

  const { error: statusError } = await client.PATCH("/admin/products/{id}/status", {
    params: { path: { id: product.id } },
    body: { status: "active" },
  });
  if (statusError) return partly("status", statusError);
  refresh();
  return {};
}

/** Publishes `product` to `channel`, adding it to what the channel already sells. */
export async function publish(channel: string, product: string): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.POST("/admin/channels/{id}/products", {
    params: { path: { id: channel } },
    body: { products: [product] },
  });
  if (error) return { error: await describe(error) };
  refresh();
  return {};
}

/** A publishable key for a storefront selling through `channel`. */
export async function createKey(channel: string, name: string): Promise<KeyResult> {
  const client = await sessionClient();
  const { data, error } = await client.POST("/admin/api-keys", {
    body: { kind: "publishable", name, channel },
  });
  if (error) return { error: await describe(error) };
  refresh();
  return { token: data.token };
}

/** Remembers the step the merchant is on, so the guide reopens there. */
export async function rememberStep(step: string): Promise<void> {
  const client = await sessionClient();
  await client.PATCH("/admin/store", { body: { setup_step: step } });
}

/** Puts the guide away (finished or skipped): it no longer shows anywhere. */
export async function finishSetup(): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.PATCH("/admin/store", { body: { setup_finished: true } });
  refresh();
  return { error: error ? await describe(error) : undefined };
}

/** Shows the guide again, from the step it was left on. */
export async function reopenSetup(): Promise<Result> {
  const client = await sessionClient();
  const { error } = await client.PATCH("/admin/store", { body: { setup_finished: false } });
  refresh();
  return { error: error ? await describe(error) : undefined };
}
