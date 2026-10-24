// Server-only: what the discount pages read besides the discount itself. The form's
// choices (markets, channels, customer groups, categories), the names of what a
// discount lists, and the live discounts the combinations card weighs.

import type { AdminSchemas } from "@hyphacommerce/sdk/admin";

import type { LiveDiscount } from "@/components/discounts/combinations-section";
import type { DiscountOptions } from "@/components/discounts/fields";
import { flatten } from "@/lib/categories";
import type { ItemSetDraft } from "@/lib/discounts/draft";
import { type Discount, draftOf } from "@/lib/discounts/wire";
import { customerName } from "@/lib/format";
import type { sessionClient } from "@/lib/hypha";

import { productSummaries } from "../markets/price-lists/grid-data";

type Client = Awaited<ReturnType<typeof sessionClient>>;
type Money = AdminSchemas["Money"];

/**
 * The form's choices. A list the role can't read is empty: markets without
 * `markets:read` (amounts are typed for none), groups without `customers:read`.
 */
export async function loadOptions(client: Client): Promise<DiscountOptions> {
  const [markets, channels, groups, categories] = await Promise.all([
    client.GET("/admin/markets"),
    client.GET("/admin/channels"),
    client.GET("/admin/customer-groups"),
    client.GET("/admin/categories"),
  ]);
  return {
    markets: (markets.data?.items ?? []).map((m) => ({
      id: m.id,
      name: m.name,
      currency: m.currency,
    })),
    channels: (channels.data?.items ?? []).map((c) => ({ value: c.id, label: c.name })),
    groups: (groups.data?.items ?? []).map((g) => ({ value: g.id, label: g.name })),
    categories: flatten(categories.data?.items ?? []).map((row) => ({
      value: row.id,
      label: row.path,
    })),
  };
}

const isClass = (c: string) => c === "product" || c === "order" || c === "shipping";

/** The live discounts (active now), at most a page of 100, for the combinations card. */
export async function loadLive(client: Client): Promise<LiveDiscount[]> {
  const { data } = await client.GET("/admin/discounts", {
    params: { query: { status: "active", limit: 100 } },
  });
  return (data?.items ?? []).map((d) => ({
    id: d.id,
    title: d.title,
    classes: d.classes.filter(isClass),
    combinesWith: d.combines_with.filter(isClass),
  }));
}

/** The most usage records added up for the usage tab's total. */
const MAX_USAGE_SUMMED = 2000;

/**
 * What a discount took off in all, by currency, from its usage records (newest first,
 * at most `MAX_USAGE_SUMMED`; `partial` past that). The API has no total of its own.
 */
export async function usageTotals(
  client: Client,
  id: string,
): Promise<{ totals: Money[]; partial: boolean }> {
  const sums = new Map<string, number>();
  let cursor: string | undefined;
  let read = 0;
  do {
    const { data } = await client.GET("/admin/discounts/{id}/usage", {
      params: { path: { id }, query: { cursor, limit: 100 } },
    });
    if (!data) break;
    for (const usage of data.items) {
      const { amount, currency } = usage.amount;
      sums.set(currency, (sums.get(currency) ?? 0) + amount);
    }
    read += data.items.length;
    cursor = data.next_cursor ?? undefined;
  } while (cursor && read < MAX_USAGE_SUMMED);
  return {
    totals: [...sums].map(([currency, amount]) => ({ amount, currency })),
    partial: cursor !== undefined,
  };
}

/** The most customers named one by one: past it, they show by ID. */
const MAX_CUSTOMER_READS = 50;
/** Variants read by ID at once: one page of the variants list. */
const VARIANTS_PER_READ = 100;

/**
 * Names of the products, variants and customers `discount` lists, by ID. A read that
 * fails leaves those by ID.
 */
export async function loadNames(
  client: Client,
  discount: Discount,
): Promise<Record<string, string>> {
  const draft = draftOf(discount, { currencies: {}, timeZone: "UTC" });
  const sets: ItemSetDraft[] = [];
  const kind = draft.kind;
  if (kind.type === "standard" && kind.product) sets.push(kind.product.items);
  if (kind.type === "buy_x_get_y") sets.push(kind.buys, kind.gets);
  if (kind.type === "tiered") sets.push(kind.items);
  const products = [...new Set(sets.flatMap((s) => s.products))];
  const variants = [...new Set(sets.flatMap((s) => s.variants))];
  const customers = draft.conditions.customers.slice(0, MAX_CUSTOMER_READS);

  const [productNames, variantNames, customerNames] = await Promise.all([
    productSummaries(client, products).then(
      (found) => found.map((p) => [p.id, p.title] as const),
      () => [],
    ),
    Promise.all(
      Array.from({ length: Math.ceil(variants.length / VARIANTS_PER_READ) }, (_, i) =>
        client.GET("/admin/variants", {
          params: {
            query: {
              ids: variants.slice(i * VARIANTS_PER_READ, (i + 1) * VARIANTS_PER_READ),
              limit: 100,
            },
          },
        }),
      ),
    ).then(
      (pages) =>
        pages.flatMap((page) =>
          (page.data?.items ?? []).map(
            (v) => [v.id, v.title ? `${v.product.title} – ${v.title}` : v.product.title] as const,
          ),
        ),
      () => [],
    ),
    Promise.all(
      customers.map((id) =>
        client
          .GET("/admin/customers/{id}", { params: { path: { id } } })
          .then(({ data }) => (data ? [[id, customerName(data) || data.email]] : [])),
      ),
    ).then(
      (found) => found.flat(),
      () => [],
    ),
  ]);
  return Object.fromEntries([...productNames, ...variantNames, ...customerNames]);
}
