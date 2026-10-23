// Server-only: one page of a listing, read through search. The category and
// search pages render it on the server, and the listing's server action reads it when
// a static category page is filtered in the browser.

import type { StorefrontPaths, StorefrontSchemas } from "@hyphacommerce/sdk/storefront";
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
      ? {
          min: major(data.price.min.amount, input.digits),
          max: major(data.price.max.amount, input.digits),
        }
      : undefined,
    nextCursor: data.next_cursor ?? null,
  };
}
