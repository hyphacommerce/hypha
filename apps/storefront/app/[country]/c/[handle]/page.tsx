import { BreadcrumbItem, Breadcrumbs } from "@astryxdesign/core/Breadcrumbs";
import { Button } from "@astryxdesign/core/Button";
import { Heading } from "@astryxdesign/core/Heading";
import { HStack, VStack } from "@astryxdesign/core/Stack";
import type { Metadata } from "next";

import { Listing } from "@/components/listing";
import { api, countryOf, loaded } from "@/lib/api";
import type { ListingParams } from "@/lib/listing";
import { link } from "@/lib/site";

type Props = {
  params: Promise<{ country: string; handle: string }>;
  searchParams: Promise<ListingParams>;
};

async function load(handle: string) {
  return loaded(
    await api().GET("/storefront/categories/by-handle/{handle}", {
      params: { path: { handle } },
    }),
  );
}

export async function generateMetadata({ params }: Props): Promise<Metadata> {
  const category = await load((await params).handle);
  return { title: category.name };
}

/**
 * A category (design note 0023, §3): its breadcrumb and subcategories, then its
 * products, those of its subcategories included, through search (0022, CA3). A hidden
 * category is a 404.
 */
export default async function CategoryPage({ params, searchParams }: Props) {
  const { country: folder, handle } = await params;
  const country = await countryOf(folder);
  const code = country.country;
  const category = await load(handle);
  const ancestors = [...category.ancestors].reverse();

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
      <Listing
        country={country}
        params={await searchParams}
        base={link(code, `/c/${handle}`)}
        category={category.id}
        productHref={(h) => link(code, `/p/${h}`)}
      />
    </VStack>
  );
}
