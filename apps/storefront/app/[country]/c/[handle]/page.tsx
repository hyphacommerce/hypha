import { BreadcrumbItem, Breadcrumbs } from "@astryxdesign/core/Breadcrumbs";
import { Button } from "@astryxdesign/core/Button";
import { Heading } from "@astryxdesign/core/Heading";
import { HStack, VStack } from "@astryxdesign/core/Stack";
import type { Metadata } from "next";
import { notFound } from "next/navigation";

import { CustomFields } from "@/components/custom-fields";
import { JsonLd } from "@/components/json-ld";
import { Listing } from "@/components/listing";
import { countryOf, fieldContext, loadCategory } from "@/lib/api";
import { presentFields } from "@/lib/custom-fields";
import { readListingPage } from "@/lib/listing-data";
import { pageMetadata } from "@/lib/metadata";
import { breadcrumbLd, plainText } from "@/lib/seo";
import { absolute, link } from "@/lib/site";

/** Static, refreshed at most every five minutes. */
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
  const { country, handle } = await params;
  const category = await load(handle);
  return pageMetadata({
    country: country.toUpperCase(),
    path: `/c/${handle}`,
    title: category.name,
    description: plainText(category.description),
    image:
      category.image?.kind === "image"
        ? { ...category.image, alt: category.image.alt ?? category.name }
        : null,
  });
}

/**
 * A category: its breadcrumb and subcategories, then its products, those of its
 * subcategories included, through search. A hidden category is a 404. The page is
 * static with its first page of products; filtering, sorting and paging read the rest
 * in the browser (`Listing`).
 */
export default async function CategoryPage({ params }: Props) {
  const { country: folder, handle } = await params;
  const [country, category] = await Promise.all([countryOf(folder), load(handle)]);
  const code = country.country;
  const ancestors = category.ancestors.toReversed();
  const [first, context] = await Promise.all([
    readListingPage({
      country: code,
      digits: country.minor_units,
      params: {},
      category: category.id,
    }),
    fieldContext(country, category.custom_fields),
  ]);
  // The merchant's custom fields shown to storefronts.
  const fields = presentFields("category", category.custom_fields, context);
  const productBase = link(code, "/p");

  return (
    <VStack gap={6}>
      <JsonLd
        data={breadcrumbLd([
          { name: "Home", url: absolute(link(code, "/")) },
          ...ancestors.map((a) => ({ name: a.name, url: absolute(link(code, `/c/${a.handle}`)) })),
          { name: category.name, url: absolute(link(code, `/c/${category.handle}`)) },
        ])}
      />
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
        {fields.length > 0 && (
          <div style={{ maxWidth: 720 }}>
            <CustomFields entries={fields} columns="multi" />
          </div>
        )}
        {category.children.length > 0 && (
          <HStack gap={2} wrap="wrap">
            {category.children.map((c) => (
              <Button key={c.id} label={c.name} href={link(code, `/c/${c.handle}`)} size="sm" />
            ))}
          </HStack>
        )}
      </VStack>
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
    </VStack>
  );
}
