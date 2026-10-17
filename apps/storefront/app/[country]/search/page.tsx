import type { Metadata } from "next";

import { Listing, type ListingParams } from "@/components/listing";
import { countryOf } from "@/lib/api";
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
  const q = typeof query.q === "string" ? query.q.trim() : "";

  return (
    <>
      <h1>{q ? `Results for “${q}”` : "All products"}</h1>
      <Listing
        country={country}
        params={query}
        base={link(code, "/search")}
        productHref={(h) => link(code, `/p/${h}`)}
      />
    </>
  );
}
