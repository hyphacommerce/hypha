// A product listing through search (design notes 0023, SF7, and 0024, §7): a category
// page and the search page are both this, with `category` or `q`. Facets and the price
// range narrow it, the sort orders it, Previous and Next page through it, all in the URL.

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { EmptyState } from "@astryxdesign/core/EmptyState";
import { HStack, VStack } from "@astryxdesign/core/Stack";

import type { Country } from "@/lib/api";
import { api } from "@/lib/api";
import {
  type ListingParams,
  changed,
  listingHref,
  nextPage,
  previousPage,
  readListing,
  sortOf,
  toMinor,
} from "@/lib/listing";

import { FacetPanel, ListingToolbar } from "./listing-controls";
import { ProductGrid } from "./product-grid";

/** Minor units as major ones for the range fields' hints ("1050" → "10.5"). */
function major(amount: number, digits: number): string {
  return String(amount / 10 ** digits);
}

export async function Listing({
  country,
  params,
  base,
  category,
  productHref,
}: {
  country: Country;
  params: ListingParams;
  /** This page's URL, without a query. */
  base: string;
  category?: string;
  productHref: (handle: string) => string;
}) {
  const state = readListing(params);
  const digits = country.minor_units;
  const sort = sortOf(state);

  const { data, error } = await api().GET("/storefront/search", {
    params: {
      query: {
        country: country.country,
        q: state.q,
        filter: state.filters,
        price_min: toMinor(state.min, digits),
        price_max: toMinor(state.max, digits),
        category,
        sort,
        cursor: state.cursor,
        limit: 24,
      },
    },
  });
  if (error) {
    // A stale cursor or filter in a hand-edited URL: start the listing again.
    return (
      <Banner
        status="warning"
        title="This listing couldn't be shown"
        description={error.error}
        endContent={<Button label="Start again" href={base} size="sm" />}
      />
    );
  }

  const facets = data.facets.filter((f) => f.values.length > 0);
  const controls = {
    base,
    state,
    facets,
    currency: country.currency,
    priceHint: data.price
      ? { min: major(data.price.min.amount, digits), max: major(data.price.max.amount, digits) }
      : undefined,
  };
  const previous = previousPage(state);
  const narrowed = state.filters.length > 0 || state.min !== undefined || state.max !== undefined;

  return (
    <div className="shop-split" data-variant="sidebar">
      <aside className="shop-wide-only" aria-label="Filters">
        <FacetPanel {...controls} />
      </aside>
      <VStack gap={5}>
        <ListingToolbar {...controls} total={data.total} sort={sort} />
        {data.items.length === 0 ? (
          <EmptyState
            title="No products here"
            description={
              narrowed ? "Nothing matches these filters." : "Nothing is for sale here yet."
            }
            actions={
              narrowed ? (
                <Button
                  label="Clear filters"
                  href={listingHref(base, changed(state, { filters: [], min: undefined, max: undefined }))}
                />
              ) : undefined
            }
          />
        ) : (
          <ProductGrid
            hits={data.items}
            digits={digits}
            hrefOf={(hit) => productHref(hit.handle)}
            minWidth={150}
          />
        )}
        {(previous || data.next_cursor) && (
          <HStack gap={2} hAlign="center">
            <Button
              label="Previous"
              href={previous ? listingHref(base, previous) : undefined}
              isDisabled={!previous}
            />
            <Button
              label="Next"
              href={data.next_cursor ? listingHref(base, nextPage(state, data.next_cursor)) : undefined}
              isDisabled={!data.next_cursor}
            />
          </HStack>
        )}
      </VStack>
    </div>
  );
}
