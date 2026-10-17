import { AspectRatio } from "@astryxdesign/core/AspectRatio";
import { Badge } from "@astryxdesign/core/Badge";
import { ClickableCard } from "@astryxdesign/core/ClickableCard";
import { Grid } from "@astryxdesign/core/Grid";
import { VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import type { StorefrontSchemas } from "@hypha/sdk/storefront";

import { formatRange } from "@/lib/money";

type Hit = StorefrontSchemas["SearchHitResponse"];

/** Search results as product cards (the `product-gallery` template): image, title, the
 * vendor, the price in the country's digits. */
export function ProductGrid({
  hits,
  digits,
  productBase,
  minWidth = 160,
}: {
  hits: Hit[];
  digits: number;
  /** Product pages' URL prefix (`/hr/p`): a string, so client components can pass it. */
  productBase: string;
  minWidth?: number;
}) {
  return (
    <Grid columns={{ minWidth, repeat: "fill" }} gap={5}>
      {hits.map((hit) => {
        const image =
          hit.image?.kind === "image" ? (hit.image.thumbnail_url ?? hit.image.url) : null;
        return (
          <ClickableCard
            key={hit.listing}
            label={hit.title}
            href={`${productBase}/${hit.handle}`}
            variant="transparent"
            padding={0}
          >
            <VStack gap={3}>
              <div className="shop-rounded">
                <AspectRatio ratio={4 / 5}>
                  {image ? (
                    // Plain <img>: the media URLs resize themselves (design note 0014).
                    <img className="shop-fill" src={image} alt={hit.image?.alt ?? ""} loading="lazy" />
                  ) : (
                    <div />
                  )}
                </AspectRatio>
              </div>
              <VStack gap={1}>
                {hit.vendor && (
                  <Text type="supporting" color="secondary">
                    {hit.vendor}
                  </Text>
                )}
                <Text type="body" weight="medium" maxLines={2}>
                  {hit.title}
                </Text>
                <Text type="body" color="secondary">
                  {formatRange(hit.listing_price.min, hit.listing_price.max, digits)}
                </Text>
                {!hit.available && (
                  <div>
                    <Badge label="Sold out" variant="neutral" />
                  </div>
                )}
              </VStack>
            </VStack>
          </ClickableCard>
        );
      })}
    </Grid>
  );
}
