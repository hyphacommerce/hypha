import { Heading } from "@astryxdesign/core/Heading";
import { VStack } from "@astryxdesign/core/Stack";
import type { Metadata } from "next";

import { Listing } from "@/components/listing";
import { countryOf } from "@/lib/api";
import { pageT } from "@/lib/i18n";
import type { ListingParams } from "@/lib/listing";
import { readListingPage } from "@/lib/listing-data";
import { link } from "@/lib/site";

/** Per request: it reads the shopper's query. */
export const dynamic = "force-dynamic";

export async function generateMetadata({
  params,
}: {
  params: Promise<{ country: string }>;
}): Promise<Metadata> {
  const t = await pageT((await params).country);
  return { title: t("search.meta_title") };
}

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

/** Search: the same listing as a category, by words. */
export default async function SearchPage({
  params,
  searchParams,
}: {
  params: Promise<{ country: string }>;
  searchParams: Promise<ListingParams>;
}) {
  const folder = (await params).country;
  const [country, t] = await Promise.all([countryOf(folder), pageT(folder)]);
  const code = country.country;
  const query = await searchParams;
  const page = await readListingPage({
    t,
    country: code,
    digits: country.minor_units,
    params: query,
  });

  return (
    <VStack gap={6}>
      <Heading level={1} type="display-2">
        {page.state.q ? t("search.results_for", { query: page.state.q }) : t("search.all_products")}
      </Heading>
      <Listing
        digits={country.minor_units}
        currency={country.currency}
        base={link(folder, "/search")}
        productBase={link(folder, "/p")}
        initial={page}
        initialQuery={queryOf(query)}
      />
    </VStack>
  );
}
