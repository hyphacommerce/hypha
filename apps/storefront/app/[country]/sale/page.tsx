import { BreadcrumbItem, Breadcrumbs } from "@astryxdesign/core/Breadcrumbs";
import { Heading } from "@astryxdesign/core/Heading";
import { VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import type { Metadata } from "next";

import { Listing } from "@/components/listing";
import { countryOf } from "@/lib/api";
import { readListingPage } from "@/lib/listing-data";
import { link } from "@/lib/site";

/** Static, refreshed at most every five minutes (and when prices change: the search
 * behind it is tagged `prices`, which a price list starting or ending expires). */
export const revalidate = 300;

/** None ahead (no API at build time): rendered on its first visit, then kept. */
export function generateStaticParams() {
  return [];
}

export const metadata: Metadata = { title: "Sale" };

/**
 * The sale shelf: everything shown with a compare-at price in the shopper's market,
 * through search's `on_sale` filter. Static like a category page; filtering, sorting and
 * paging read the rest in the browser (`Listing`), on sale always.
 */
export default async function SalePage({ params }: { params: Promise<{ country: string }> }) {
  const country = await countryOf((await params).country);
  const code = country.country;
  const first = await readListingPage({
    country: code,
    digits: country.minor_units,
    params: {},
    onSale: true,
  });

  return (
    <VStack gap={6}>
      <VStack gap={3}>
        <Breadcrumbs variant="supporting">
          <BreadcrumbItem href={link(code, "/")}>Home</BreadcrumbItem>
          <BreadcrumbItem isCurrent>Sale</BreadcrumbItem>
        </Breadcrumbs>
        <Heading level={1} type="display-2">
          Sale
        </Heading>
        <Text color="secondary">Reduced prices, while they last.</Text>
      </VStack>
      <Listing
        country={code}
        digits={country.minor_units}
        currency={country.currency}
        base={link(code, "/sale")}
        productBase={link(code, "/p")}
        onSale
        initial={first}
        initialQuery=""
      />
    </VStack>
  );
}
