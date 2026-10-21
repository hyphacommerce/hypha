import { AspectRatio } from "@astryxdesign/core/AspectRatio";
import { Badge } from "@astryxdesign/core/Badge";
import { Grid } from "@astryxdesign/core/Grid";
import { VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import type { StorefrontSchemas } from "@hypha/sdk/storefront";

import { formatRange } from "@/lib/money";

import { SiteLink as Link } from "./site-link";

type Hit = StorefrontSchemas["SearchHitResponse"];

/**
 * Search results as product cards (the `product-gallery` template): image, title, the
 * vendor, the price in the country's digits. Each card is one `next/link` anchor
 * (`.shop-product-card`, globals.css), so Next prefetches it as it scrolls into view
 * and on hover, and the product page is there when it's clicked. (Astryx's ClickableCard
 * hides its link in a 1px element, which the viewport prefetch never sees.)
 */
export function ProductGrid({
  hits,
  digits,
  productBase,
  minWidth = 160,
  eager = 0,
}: {
  hits: Hit[];
  digits: number;
  /** Product pages' URL prefix (`/hr/p`): a string, so client components can pass it. */
  productBase: string;
  minWidth?: number;
  /** How many of the first images load at once, with priority: the ones above the
   * fold on a page's first render (its largest paint); the rest load lazily. */
  eager?: number;
}) {
  return (
    <Grid columns={{ minWidth, repeat: "fill" }} gap={5}>
      {hits.map((hit, i) => {
        const image =
          hit.image?.kind === "image" ? (hit.image.thumbnail_url ?? hit.image.url) : null;
        const first = i < eager;
        return (
          <Link
            key={hit.listing}
            href={`${productBase}/${hit.handle}`}
            className="shop-product-card"
            aria-label={hit.title}
          >
            <VStack gap={3}>
              <div className="shop-rounded shop-product-image">
                <AspectRatio ratio={4 / 5}>
                  {image ? (
                    // Plain <img>: the media URLs resize themselves (design note 0014).
                    <img
                      className="shop-fill"
                      src={image}
                      alt={hit.image?.alt ?? ""}
                      loading={first ? "eager" : "lazy"}
                      fetchPriority={first ? "high" : undefined}
                      decoding="async"
                    />
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
          </Link>
        );
      })}
    </Grid>
  );
}
