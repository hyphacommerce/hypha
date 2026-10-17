import { Heading } from "@astryxdesign/core/Heading";
import { VStack } from "@astryxdesign/core/Stack";
import type { Metadata } from "next";
import { Suspense } from "react";

import { Listing } from "@/components/listing";
import { countryOf } from "@/lib/api";
import type { ListingParams } from "@/lib/listing";
import { readListingPage } from "@/lib/listing-data";
import { link } from "@/lib/site";

/** Per request: it reads the shopper's query (design note 0024, §7). */
export const dynamic = "force-dynamic";

export const metadata: Metadata = { title: "Search" };

/** The listing parameters of `params`, as `Listing` keys them, to skip a second read. */
function queryOf(params: ListingParams): string {
  const kept = new URLSearchParams();
  for (const key of ["q", "filter", "min", "max", "sort", "cursor", "back"]) {
    const value = params[key];
    for (const v of value === undefined ? [] : Array.isArray(value) ? value : [value]) {
      kept.append(key, v);
    }
  }
  return kept.toString();
}

/** Search (design note 0023, §3): the same listing as a category, by words. */
export default async function SearchPage({
  params,
  searchParams,
}: {
  params: Promise<{ country: string }>;
  searchParams: Promise<ListingParams>;
}) {
  const country = await countryOf((await params).country);
  const code = country.country;
  const query = await searchParams;
  const page = await readListingPage({ country: code, digits: country.minor_units, params: query });

  return (
    <VStack gap={6}>
      <Heading level={1} type="display-2">
        {page.state.q ? `Results for “${page.state.q}”` : "All products"}
      </Heading>
      <Suspense>
        <Listing
          country={code}
          digits={country.minor_units}
          currency={country.currency}
          base={link(code, "/search")}
          productBase={link(code, "/p")}
          initial={page}
          initialQuery={queryOf(query)}
        />
      </Suspense>
    </VStack>
  );
}
