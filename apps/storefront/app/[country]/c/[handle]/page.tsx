import { BreadcrumbItem, Breadcrumbs } from "@astryxdesign/core/Breadcrumbs";
import { Button } from "@astryxdesign/core/Button";
import { Heading } from "@astryxdesign/core/Heading";
import { HStack, VStack } from "@astryxdesign/core/Stack";
import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { Suspense } from "react";

import { Listing } from "@/components/listing";
import { ProductGrid } from "@/components/product-grid";
import { countryOf, loadCategory } from "@/lib/api";
import { readListingPage } from "@/lib/listing-data";
import { link } from "@/lib/site";

/** Static, refreshed at most every five minutes (design note 0024, §7). */
export const revalidate = 300;

/** None ahead (no API at build time): each category renders on its first visit, then
 * is kept and refreshed in the background. */
export function generateStaticParams() {
  return [];
}

type Props = {
  params: Promise<{ country: string; handle: string }>;
};

async function load(handle: string) {
  const category = await loadCategory(handle);
  if (!category) notFound();
  return category;
}

export async function generateMetadata({ params }: Props): Promise<Metadata> {
  const category = await load((await params).handle);
  return { title: category.name };
}

/**
 * A category (design note 0023, §3): its breadcrumb and subcategories, then its
 * products, those of its subcategories included, through search (0022, CA3). A hidden
 * category is a 404. The page is static with its first page of products; filtering,
 * sorting and paging read the rest in the browser (`Listing`).
 */
export default async function CategoryPage({ params }: Props) {
  const { country: folder, handle } = await params;
  const country = await countryOf(folder);
  const code = country.country;
  const category = await load(handle);
  const ancestors = [...category.ancestors].reverse();
  const first = await readListingPage({
    country: code,
    digits: country.minor_units,
    params: {},
    category: category.id,
  });
  const productBase = link(code, "/p");

  return (
    <VStack gap={6}>
      <VStack gap={3}>
        <Breadcrumbs variant="supporting">
          <BreadcrumbItem href={link(code, "/")}>Home</BreadcrumbItem>
          {ancestors.map((a) => (
            <BreadcrumbItem key={a.id} href={link(code, `/c/${a.handle}`)}>
              {a.name}
            </BreadcrumbItem>
          ))}
          <BreadcrumbItem isCurrent>{category.name}</BreadcrumbItem>
        </Breadcrumbs>
        <Heading level={1} type="display-2">
          {category.name}
        </Heading>
        {category.description && (
          <div
            className="shop-prose"
            style={{ maxWidth: 720 }}
            // Sanitised by the API (scripts, styles and event attributes removed).
            dangerouslySetInnerHTML={{ __html: category.description }}
          />
        )}
        {category.children.length > 0 && (
          <HStack gap={2} wrap="wrap">
            {category.children.map((c) => (
              <Button key={c.id} label={c.name} href={link(code, `/c/${c.handle}`)} size="sm" />
            ))}
          </HStack>
        )}
      </VStack>
      {/* The URL's query is only known in the browser on a static page. */}
      <Suspense
        fallback={<ProductGrid hits={first.items} digits={country.minor_units} productBase={productBase} />}
      >
        <Listing
          country={code}
          digits={country.minor_units}
          currency={country.currency}
          base={link(code, `/c/${handle}`)}
          productBase={productBase}
          category={category.id}
          initial={first}
          initialQuery=""
        />
      </Suspense>
    </VStack>
  );
}
