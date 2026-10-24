// Server-only: one page of a listing, read through search. The category and
// search pages render it on the server, and the listing's server action reads it when
// a static category page is filtered in the browser.

import type { StorefrontPaths, StorefrontSchemas } from "@hyphacommerce/sdk/storefront";
import { unstable_cache } from "next/cache";

import { api } from "./api";
import { FRESH_SECONDS, TAGS } from "./cache-tags";
import {
  ON_SALE,
  type ListingParams,
  type ListingState,
  readListing,
  sortOf,
  toMinor,
} from "./listing";
import { rankRelated } from "./related";

export type ListingPage = {
  state: ListingState;
  items: StorefrontSchemas["SearchHitResponse"][];
  total: number;
  /** Without `on_sale`: the listing's on-sale toggle stands for it. */
  facets: StorefrontSchemas["FacetResponse"][];
  /** How many of the results are on sale, when the merchant counts it (`on_sale` among
   * the search facets). */
  onSaleCount?: number;
  /** The results' price range in major units, for the range fields' hints. */
  priceHint?: { min: string; max: string };
  nextCursor: string | null;
  /** Why the API refused the URL's listing (a stale cursor, a bad filter). */
  error?: string;
};

type SearchQuery = StorefrontPaths["/storefront/search"]["get"]["parameters"]["query"];
type SearchResult =
  | { data: StorefrontSchemas["SearchResponse"]; error?: undefined }
  | { data?: undefined; error: { code: string; error: string } };

/** A search, as the API answers it: its results, or its refusal of the query (a stale
 * cursor, a bad filter: the listing says why). A failure (the API down, a 500, a rate
 * limit) throws instead, so the cached search below keeps nothing and asks again. */
async function searchNow(query: SearchQuery): Promise<SearchResult> {
  const { data, error, response } = await api().GET("/storefront/search", {
    params: { query },
  });
  if (data) return { data };
  const refused = response.status >= 400 && response.status < 500 && response.status !== 429;
  if (!refused || !error) throw new Error(`${response.url} failed with ${response.status}`);
  return { error };
}

/**
 * A search, cached and tagged (`listings`, `prices`) so a static page showing it is
 * re-rendered when the webhook says products or prices changed. Free text (`q`) isn't
 * cached: shoppers' words are mostly distinct, so each would only add an entry (a KV
 * write on Workers) that hardly anyone reads again; the search page that shows them is
 * dynamic anyway.
 */
export function search(query: SearchQuery): Promise<SearchResult> {
  if (query.q?.trim()) return searchNow(query);
  return unstable_cache(() => searchNow(query), ["search", JSON.stringify(query)], {
    tags: [TAGS.listings, TAGS.prices],
    revalidate: FRESH_SECONDS,
  })();
}

/** Minor units as major ones ("1050" → "10.5"). */
function major(amount: number, digits: number): string {
  return String(amount / 10 ** digits);
}

export async function readListingPage(input: {
  country: string;
  digits: number;
  params: ListingParams;
  category?: string;
  /** Only what's on sale, whatever the URL says (the sale page). */
  onSale?: boolean;
}): Promise<ListingPage> {
  const state = readListing(input.params);
  const filter =
    input.onSale && !state.filters.includes(ON_SALE) ? [...state.filters, ON_SALE] : state.filters;
  const { data, error } = await search({
    country: input.country,
    q: state.q,
    filter,
    price_min: toMinor(state.min, input.digits),
    price_max: toMinor(state.max, input.digits),
    category: input.category,
    sort: sortOf(state),
    cursor: state.cursor,
    limit: 24,
  });
  if (error) {
    return { state, items: [], total: 0, facets: [], nextCursor: null, error: error.error };
  }
  return {
    state,
    items: data.items,
    total: data.total,
    facets: data.facets.filter((f) => f.values.length > 0 && f.key !== "on_sale"),
    onSaleCount: data.facets
      .find((f) => f.key === "on_sale")
      ?.values.find((v) => v.value === "true")?.count,
    priceHint: data.price
      ? {
          min: major(data.price.min.amount, input.digits),
          max: major(data.price.max.amount, input.digits),
        }
      : undefined,
    nextCursor: data.next_cursor ?? null,
  };
}

/** A related shelf's length: one row on a wide screen. */
const RELATED = 4;

/**
 * Products to show beside `product` in `country` (`rankRelated`): those sharing a tag,
 * its type or its maker, and the newest to fill up, from four cached searches made at
 * once. A search that fails leaves its part out: the shelf is extra, never the page's
 * failure.
 */
export async function relatedProducts(
  product: { id: string; tags: string[]; product_type?: string | null; vendor?: string | null },
  country: string,
): Promise<ListingPage["items"]> {
  const hits = (filter: string[] | null) =>
    filter === null
      ? Promise.resolve([])
      : search({ country, filter, sort: "newest", limit: 12 }).then(
          (r) => r.data?.items ?? [],
          () => [],
        );
  const [tags, type, vendor, newest] = await Promise.all([
    hits(product.tags.length > 0 ? product.tags.map((t) => `tag:${t}`) : null),
    hits(product.product_type ? [`product_type:${product.product_type}`] : null),
    hits(product.vendor ? [`vendor:${product.vendor}`] : null),
    hits([]),
  ]);
  return rankRelated(product, { tags, type, vendor, newest }, RELATED);
}
