import { Button } from "@astryxdesign/core/Button";
import { ClickableCard } from "@astryxdesign/core/ClickableCard";
import { EmptyState } from "@astryxdesign/core/EmptyState";
import { Grid } from "@astryxdesign/core/Grid";
import { Heading } from "@astryxdesign/core/Heading";
import { HStack, VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";

import { ProductGrid } from "@/components/product-grid";
import { api, countryOf, loadCategories, loadStore } from "@/lib/api";
import { link } from "@/lib/site";

/** Home (design note 0023, §3): the store, its top categories, its newest products. */
export default async function HomePage({ params }: { params: Promise<{ country: string }> }) {
  const country = await countryOf((await params).country);
  const code = country.country;
  const [store, categories, newest] = await Promise.all([
    loadStore(),
    loadCategories(),
    api().GET("/storefront/search", {
      params: { query: { country: code, sort: "newest", limit: 8 } },
    }),
  ]);
  const products = newest.data?.items ?? [];

  return (
    <VStack gap={10}>
      <VStack gap={4} paddingBlock={6} maxWidth={720}>
        <Heading level={1} type="display-1">
          {store.name}
        </Heading>
        {store.description && (
          <Text type="large" color="secondary">
            {store.description}
          </Text>
        )}
        <div>
          <Button label="Shop everything" href={link(code, "/search")} variant="primary" size="lg" />
        </div>
      </VStack>

      {categories.length > 0 && (
        <VStack gap={4}>
          <Heading level={2}>Shop by category</Heading>
          <Grid columns={{ minWidth: 180, repeat: "fill" }} gap={3}>
            {categories.map((c) => (
              <ClickableCard
                key={c.id}
                label={c.name}
                href={link(code, `/c/${c.handle}`)}
                variant="muted"
                padding={5}
              >
                <Text type="large" weight="medium">
                  {c.name}
                </Text>
              </ClickableCard>
            ))}
          </Grid>
        </VStack>
      )}

      <VStack gap={4}>
        <HStack hAlign="between" vAlign="center">
          <Heading level={2}>New in</Heading>
          {products.length > 0 && (
            <Button label="View all" href={link(code, "/search?sort=newest")} variant="ghost" />
          )}
        </HStack>
        {products.length === 0 ? (
          <EmptyState
            title="Nothing for sale here yet"
            description={`Products sold in ${country.name} will show up here.`}
          />
        ) : (
          <ProductGrid
            hits={products}
            digits={country.minor_units}
            hrefOf={(hit) => link(code, `/p/${hit.handle}`)}
          />
        )}
      </VStack>
    </VStack>
  );
}
