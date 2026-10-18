"use client";

// A product listing through search (design notes 0023, SF7, and 0024, §7): a category
// page and the search page are both this, with `category` or `q`. Facets and the price
// range narrow it, the sort orders it, Previous and Next page through it, all in the
// URL. A category page is static, so it renders its first page; when the URL asks for
// another (a filter, a sort, a page), this reads it through `listingPage`.

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { EmptyState } from "@astryxdesign/core/EmptyState";
import { HStack, StackItem, VStack } from "@astryxdesign/core/Stack";
import { useMediaQuery } from "@astryxdesign/core/hooks";
import { useSearchParams } from "next/navigation";
import { Suspense, useEffect, useState, useTransition } from "react";

import { listingPage } from "@/app/[country]/actions";
import { changed, listingHref, nextPage, previousPage, sortOf } from "@/lib/listing";
import type { ListingPage } from "@/lib/listing-data";

import { FacetPanel, ListingToolbar } from "./listing-controls";
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
  // Facets beside the results on wide screens; on phones the toolbar's Filters button
  // opens them in a bottom sheet.
  const isWide = useMediaQuery("(min-width: 900px)", true);
  const [shown, setShown] = useState({ query: initialQuery, page: initial });
  const [isLoading, startLoading] = useTransition();
  // From the URL changing until its results are here, the old results stay, dimmed: a
  // grid of skeletons on every filter click makes the page jump.
  const isBusy = isLoading || query !== shown.query;

  useEffect(() => {
    if (query === shown.query) return;
    if (query === initialQuery) {
      setShown({ query, page: initial });
      return;
    }
    startLoading(async () => {
      const page = await listingPage({ country, category, query });
      setShown({ query, page });
    });
  }, [query, shown.query, initialQuery, initial, country, category]);

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
  const narrowed = state.filters.length > 0 || state.min !== undefined || state.max !== undefined;

  return (
    <HStack gap={8} vAlign="start">
      {watch}
      {isWide && (
        <VStack width={240} gap={4}>
          <FacetPanel {...controls} />
        </VStack>
      )}
      <StackItem size="fill">
        <VStack gap={5}>
          <ListingToolbar {...controls} total={page.total} sort={sortOf(state)} />
          <div
            aria-busy={isBusy}
            style={{
              opacity: isBusy ? 0.5 : 1,
              transition: "opacity 120ms",
              pointerEvents: isBusy ? "none" : undefined,
            }}
          >
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
                      href={listingHref(
                        base,
                        changed(state, { filters: [], min: undefined, max: undefined }),
                      )}
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
              />
            )}
          </div>
          {(previous || page.nextCursor) && (
            <HStack gap={2} hAlign="center">
              <Button
                label="Previous"
                href={previous ? listingHref(base, previous) : undefined}
                isDisabled={!previous}
              />
              <Button
                label="Next"
                href={
                  page.nextCursor ? listingHref(base, nextPage(state, page.nextCursor)) : undefined
                }
                isDisabled={!page.nextCursor}
              />
            </HStack>
          )}
        </VStack>
      </StackItem>
    </HStack>
  );
}
