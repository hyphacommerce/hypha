import { AspectRatio } from "@astryxdesign/core/AspectRatio";
import { Button } from "@astryxdesign/core/Button";
import { ClickableCard } from "@astryxdesign/core/ClickableCard";
import { EmptyState } from "@astryxdesign/core/EmptyState";
import { Grid } from "@astryxdesign/core/Grid";
import { Heading } from "@astryxdesign/core/Heading";
import { Section } from "@astryxdesign/core/Section";
import { HStack, VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import { ArrowRightIcon } from "@heroicons/react/20/solid";
import type { StorefrontSchemas } from "@hypha/sdk/storefront";

import { ProductGrid } from "@/components/product-grid";
import { countryOf, loadCategories, loadStore } from "@/lib/api";
import { search } from "@/lib/listing-data";
import { link } from "@/lib/site";

type Hit = StorefrontSchemas["SearchHitResponse"];

/** Static, refreshed at most every five minutes (the country layout's `revalidate`). */
async function shelf(query: {
  country: string;
  sort: "newest" | "price_asc";
  limit: number;
  category?: string;
}): Promise<Hit[]> {
  const { data } = await search(query);
  return data?.items ?? [];
}

/** A row of products under a heading, with a link to all of them. */
function Shelf({
  title,
  description,
  href,
  hits,
  digits,
  productBase,
}: {
  title: string;
  description?: string;
  href: string;
  hits: Hit[];
  digits: number;
  productBase: string;
}) {
  if (hits.length === 0) return null;
  return (
    <VStack gap={4}>
      <HStack hAlign="between" vAlign="end" gap={4}>
        <VStack gap={1}>
          <Heading level={2}>{title}</Heading>
          {description && <Text color="secondary">{description}</Text>}
        </VStack>
        <Button
          label="View all"
          href={href}
          variant="ghost"
          endContent={<ArrowRightIcon width={16} height={16} aria-hidden />}
        />
      </HStack>
      <ProductGrid hits={hits} digits={digits} productBase={productBase} minWidth={200} />
    </VStack>
  );
}

/**
 * Home, static: the store, its categories, what's new, a shelf per top category and
 * the best prices, all through search, so each is priced and stocked for the country.
 */
export default async function HomePage({ params }: { params: Promise<{ country: string }> }) {
  // Every read at once: the folder is the country's code (the proxy lets only sold-to
  // countries' folders through), and the category shelves start as soon as the
  // categories are in, without waiting for the other reads.
  const folder = (await params).country;
  const code = folder.toUpperCase();
  const [country, store, categories, newest, value, shelves] = await Promise.all([
    countryOf(folder),
    loadStore(),
    loadCategories(),
    shelf({ country: code, sort: "newest", limit: 8 }),
    shelf({ country: code, sort: "price_asc", limit: 4 }),
    loadCategories().then((all) =>
      Promise.all(
        all
          .slice(0, 3)
          .map((c) => shelf({ country: code, sort: "newest", limit: 4, category: c.id })),
      ),
    ),
  ]);
  const featured = categories.slice(0, 3);
  const digits = country.minor_units;
  const productBase = link(code, "/p");
  const hero = newest.find((hit) => hit.image?.kind === "image")?.image;

  if (newest.length === 0) {
    return (
      <EmptyState
        title={`Welcome to ${store.name}`}
        description={`Products sold in ${country.name} will show up here.`}
      />
    );
  }

  return (
    <VStack gap={10}>
      <VStack gap={8}>
        <VStack gap={5} hAlign="center" paddingBlock={6}>
          <VStack gap={3} hAlign="center" maxWidth={720}>
            <Heading level={1} type="display-1" justify="center" textWrap="balance">
              {store.name}
            </Heading>
            {store.description && (
              <Text type="large" color="secondary" justify="center" textWrap="balance">
                {store.description}
              </Text>
            )}
          </VStack>
          <HStack gap={3} hAlign="center" wrap="wrap">
            <Button
              label="Shop everything"
              href={link(code, "/search")}
              variant="primary"
              size="lg"
              endContent={<ArrowRightIcon width={16} height={16} aria-hidden />}
            />
            {featured[0] && (
              <Button
                label={`Browse ${featured[0].name}`}
                href={link(code, `/c/${featured[0].handle}`)}
                variant="secondary"
                size="lg"
              />
            )}
          </HStack>
        </VStack>
        {hero && (
          <Section variant="transparent" padding={0}>
            <div className="shop-rounded">
              <AspectRatio ratio={3 / 1}>
                <img
                  className="shop-fill"
                  src={hero.url}
                  alt={hero.alt ?? ""}
                  fetchPriority="high"
                />
              </AspectRatio>
            </div>
          </Section>
        )}
      </VStack>

      {categories.length > 0 && (
        <VStack gap={4}>
          <Heading level={2}>Shop by category</Heading>
          <Grid columns={{ minWidth: 200, repeat: "fill" }} gap={3}>
            {categories.map((c) => (
              <ClickableCard
                key={c.id}
                label={c.name}
                href={link(code, `/c/${c.handle}`)}
                variant="muted"
                padding={6}
              >
                <VStack gap={1}>
                  <Text type="large" weight="semibold">
                    {c.name}
                  </Text>
                  <Text type="supporting" color="secondary">
                    {c.children.length > 0
                      ? c.children.map((child) => child.name).join(", ")
                      : "Shop now"}
                  </Text>
                </VStack>
              </ClickableCard>
            ))}
          </Grid>
        </VStack>
      )}

      <Shelf
        title="New arrivals"
        description="The latest additions to the store."
        href={link(code, "/search?sort=newest")}
        hits={newest}
        digits={digits}
        productBase={productBase}
      />

      {featured.map((c, i) => (
        <Shelf
          key={c.id}
          title={c.name}
          href={link(code, `/c/${c.handle}`)}
          hits={shelves[i]}
          digits={digits}
          productBase={productBase}
        />
      ))}

      <Section variant="muted" padding={8}>
        <Shelf
          title="Best prices"
          description={`Our most affordable picks in ${country.currency}.`}
          href={link(code, "/search?sort=price_asc")}
          hits={value}
          digits={digits}
          productBase={productBase}
        />
      </Section>
    </VStack>
  );
}
