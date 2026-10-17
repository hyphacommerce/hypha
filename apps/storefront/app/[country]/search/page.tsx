import { Heading } from "@astryxdesign/core/Heading";
import { VStack } from "@astryxdesign/core/Stack";
import type { Metadata } from "next";

import { Listing } from "@/components/listing";
import { countryOf } from "@/lib/api";
import { type ListingParams, readListing } from "@/lib/listing";
import { link } from "@/lib/site";

export const metadata: Metadata = { title: "Search" };

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
  const { q } = readListing(query);

  return (
    <VStack gap={6}>
      <Heading level={1} type="display-2">
        {q ? `Results for “${q}”` : "All products"}
      </Heading>
      <Listing
        country={country}
        params={query}
        base={link(code, "/search")}
        productHref={(h) => link(code, `/p/${h}`)}
      />
    </VStack>
  );
}
