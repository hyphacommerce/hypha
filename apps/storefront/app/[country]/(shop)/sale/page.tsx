import { BreadcrumbItem, Breadcrumbs } from "@astryxdesign/core/Breadcrumbs";
import { Heading } from "@astryxdesign/core/Heading";
import { VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import type { Metadata } from "next";

import { Listing } from "@/components/listing";
import { countryOf, pageLocale } from "@/lib/api";
import { pageT } from "@/lib/i18n";
import { readListingPage } from "@/lib/listing-data";
import { pageMetadata } from "@/lib/metadata";
import { link } from "@/lib/site";

/** Static, refreshed at most every five minutes (and when prices change: the search
 * behind it is tagged `prices`, which a price list starting or ending expires). */
export const revalidate = 300;

/** None ahead (no API at build time): rendered on its first visit, then kept. */
export function generateStaticParams() {
  return [];
}

export async function generateMetadata({
  params,
}: {
  params: Promise<{ country: string }>;
}): Promise<Metadata> {
  const folder = (await params).country;
  const t = await pageT(folder);
  return pageMetadata({
    segment: folder,
    path: "/sale",
    title: t("sale.title"),
    description: t("sale.meta_description"),
  });
}

/**
 * The sale shelf: everything shown with a compare-at price in the shopper's market,
 * through search's `on_sale` filter. Static like a category page; filtering, sorting and
 * paging read the rest in the browser (`Listing`), on sale always.
 */
export default async function SalePage({ params }: { params: Promise<{ country: string }> }) {
  const folder = (await params).country;
  const [country, place, t] = await Promise.all([
    countryOf(folder),
    pageLocale(folder),
    pageT(folder),
  ]);
  const first = await readListingPage({
    t,
    place,
    digits: country.minor_units,
    params: {},
    onSale: true,
  });

  return (
    <VStack gap={6}>
      <VStack gap={3}>
        <Breadcrumbs variant="supporting">
          <BreadcrumbItem href={link(folder, "/")}>{t("common.home")}</BreadcrumbItem>
          <BreadcrumbItem isCurrent>{t("sale.title")}</BreadcrumbItem>
        </Breadcrumbs>
        <Heading level={1} type="display-2">
          {t("sale.title")}
        </Heading>
        <Text color="secondary">{t("sale.intro")}</Text>
      </VStack>
      <Listing
        digits={country.minor_units}
        currency={country.currency}
        base={link(folder, "/sale")}
        productBase={link(folder, "/p")}
        onSale
        initial={first}
        initialQuery=""
      />
    </VStack>
  );
}
