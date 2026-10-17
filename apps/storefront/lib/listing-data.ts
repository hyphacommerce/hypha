// Server-only: one page of a listing, read through search (design notes 0023, SF7, and
// 0024, §7). The category and search pages render it on the server, and the listing's
// server action reads it when a static category page is filtered in the browser.

import type { StorefrontPaths, StorefrontSchemas } from "@hypha/sdk/storefront";
import { unstable_cache } from "next/cache";

import { api } from "./api";
import { FRESH_SECONDS, TAGS } from "./cache-tags";
import { type ListingParams, type ListingState, readListing, sortOf, toMinor } from "./listing";

export type ListingPage = {
  state: ListingState;
  items: StorefrontSchemas["SearchHitResponse"][];
  total: number;
  facets: StorefrontSchemas["FacetResponse"][];
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

/**
 * A search, cached and tagged (`listings`, `prices`) so a static page showing it is
 * re-rendered when the webhook says products or prices changed.
 */
export function search(query: SearchQuery): Promise<SearchResult> {
  return unstable_cache(
    async (): Promise<SearchResult> => {
      const { data, error } = await api().GET("/storefront/search", { params: { query } });
      return data ? { data } : { error: error ?? { code: "unknown", error: "search failed" } };
    },
    ["search", JSON.stringify(query)],
    { tags: [TAGS.listings, TAGS.prices], revalidate: FRESH_SECONDS },
  )();
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
}): Promise<ListingPage> {
  const state = readListing(input.params);
  const { data, error } = await search({
    country: input.country,
    q: state.q,
    filter: state.filters,
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
    facets: data.facets.filter((f) => f.values.length > 0),
    priceHint: data.price
      ? { min: major(data.price.min.amount, input.digits), max: major(data.price.max.amount, input.digits) }
      : undefined,
    nextCursor: data.next_cursor ?? null,
  };
}
