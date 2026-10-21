"use client";

// A product listing through search (design notes 0023, SF7, and 0024, §7): a category
// page and the search page are both this, with `category` or `q`. Facets and the price
// range narrow it, the sort orders it, Previous and Next page through it, all in the
// URL. The page (static for a category) renders its first page; when the URL asks for
// another (a filter, a sort, a page: changed in place by the controls, `go`), this reads
// it through `listingPage`: one call, with the old results dimmed until the new ones are
// here. Facets beside the results on wide screens, in the toolbar's sheet on phones, by
// CSS (`.shop-listing`), so the HTML is the same for both and nothing moves on load.

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { EmptyState } from "@astryxdesign/core/EmptyState";
import { HStack, VStack } from "@astryxdesign/core/Stack";
import { useSearchParams } from "next/navigation";
import { Suspense, useEffect, useRef, useState, useTransition } from "react";

import { listingPage } from "@/app/[country]/actions";
import { type ListingState, changed, nextPage, previousPage, sortOf } from "@/lib/listing";
import type { ListingPage } from "@/lib/listing-data";

import { FacetPanel, ListingToolbar, go } from "./listing-controls";
import { ProductGrid } from "./product-grid";

/** The URL's listing parameters, in a stable order, to tell one listing from another. */
function listingQuery(params: URLSearchParams): string {
  const keys = ["q", "filter", "min", "max", "sort", "cursor", "back"];
  const kept = new URLSearchParams();
  for (const key of keys) for (const value of params.getAll(key)) kept.append(key, value);
  return kept.toString();
}

/**
 * Reports the URL's listing query, rendering nothing. Reading the query on a static page
 * makes everything up to the nearest Suspense boundary render in the browser only, so
 * only this does, inside its own boundary with no fallback: the listing itself is in the
 * page's static HTML from the start, with the server's first page.
 */
function QueryWatch({ onQuery }: { onQuery: (query: string) => void }) {
  const query = listingQuery(useSearchParams());
  // Up to the parent on purpose: the parent can't read the query itself without leaving
  // the static HTML.
  // react-doctor-disable-next-line react-doctor/no-pass-data-to-parent
  useEffect(() => onQuery(query), [query, onQuery]);
  return null;
}

export function Listing({
  country,
  digits,
  currency,
  base,
  productBase,
  category,
  initial,
  initialQuery,
}: {
  country: string;
  digits: number;
  currency: string;
  /** This page's URL, without a query. */
  base: string;
  /** Product pages' URL prefix (`/hr/p`). */
  productBase: string;
  category?: string;
  /** The page the server rendered, for `initialQuery`. */
  initial: ListingPage;
  initialQuery: string;
}) {
  // The page the server rendered is for `initialQuery`; the URL's own comes once the
  // page is in the browser (`QueryWatch`).
  const [query, setQuery] = useState(initialQuery);
  const watch = (
    <Suspense fallback={null}>
      <QueryWatch onQuery={setQuery} />
    </Suspense>
  );
  const [shown, setShown] = useState({ query: initialQuery, page: initial });
  const [isLoading, startLoading] = useTransition();
  // Back to the server's query: its page is here already (adjusting state while
  // rendering, so React re-renders before showing the stale results).
  if (query === initialQuery && shown.query !== initialQuery) {
    setShown({ query, page: initial });
  }

  // From the URL changing until its results are here, the old results stay, dimmed: a
  // grid of skeletons on every filter click makes the page jump.
  const isBusy = isLoading || query !== shown.query;

  useEffect(() => {
    if (query === shown.query || query === initialQuery) return;
    startLoading(async () => {
      const page = await listingPage({ country, category, query });
      setShown({ query, page });
    });
  }, [query, shown.query, initialQuery, country, category]);

  // Paging starts the shopper at the top of the results, as a new page would.
  const top = useRef<HTMLDivElement>(null);
  const turn = (state: ListingState) => {
    go(base, state);
    top.current?.scrollIntoView({ block: "start" });
  };

  const page = shown.page;
  const { state } = page;
  if (page.error) {
    return (
      <>
        {watch}
        <Banner
          status="warning"
          title="This listing couldn't be shown"
          description={page.error}
          endContent={<Button label="Start again" href={base} size="sm" />}
        />
      </>
    );
  }
  const controls = { base, state, facets: page.facets, currency, priceHint: page.priceHint };
  const previous = previousPage(state);
  const next = page.nextCursor;
  const narrowed = state.filters.length > 0 || state.min !== undefined || state.max !== undefined;

  return (
    <div className="shop-listing" ref={top}>
      {watch}
      <aside className="shop-listing-facets" aria-label="Filters">
        <FacetPanel {...controls} />
      </aside>
      <VStack gap={5}>
        <ListingToolbar {...controls} total={page.total} sort={sortOf(state)} isBusy={isBusy} />
        <div className="shop-listing-results" aria-busy={isBusy} data-busy={isBusy || undefined}>
          {page.items.length === 0 ? (
            <EmptyState
              title="No products here"
              description={
                narrowed ? "Nothing matches these filters." : "Nothing is for sale here yet."
              }
              actions={
                narrowed ? (
                  <Button
                    label="Clear filters"
                    clickAction={() =>
                      go(base, changed(state, { filters: [], min: undefined, max: undefined }))
                    }
                  />
                ) : undefined
              }
            />
          ) : (
            <ProductGrid
              hits={page.items}
              digits={digits}
              productBase={productBase}
              minWidth={180}
              eager={shown.query === initialQuery ? 4 : 0}
            />
          )}
        </div>
        {(previous || next) && (
          <HStack gap={2} hAlign="center">
            <Button
              label="Previous"
              clickAction={previous ? () => turn(previous) : undefined}
              isDisabled={!previous}
            />
            <Button
              label="Next"
              clickAction={next ? () => turn(nextPage(state, next)) : undefined}
              isDisabled={!next}
            />
          </HStack>
        )}
      </VStack>
    </div>
  );
}
