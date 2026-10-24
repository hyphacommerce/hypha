import { AspectRatio } from "@astryxdesign/core/AspectRatio";
import { Badge } from "@astryxdesign/core/Badge";
import { Grid } from "@astryxdesign/core/Grid";
import { HStack, VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import type { StorefrontSchemas } from "@hyphacommerce/sdk/storefront";
import { readPromotions } from "@/lib/discounts";
import { formatRange } from "@/lib/money";

import { PromotionBadges } from "./discounts";
import { Price, SaleBadge } from "./price";
import { SiteLink as Link } from "./site-link";
import { ViewTransition } from "./view-transition";

type Hit = StorefrontSchemas["SearchHitResponse"];

/** The shared element between a product's card and its page: the image. */
export function productImageName(handle: string): string {
  return `product-${handle}`;
}

/**
 * Search results as product cards (the `product-gallery` template): image, title, the
 * vendor, the price in the country's digits (the variant shown's compare-at struck
 * through beside it, with a Sale badge, when it's on sale), and its promotions as badges
 * ("Buy 2, get 1 free"). Each card is one `next/link` anchor
 * (`.shop-product-card`, globals.css), so Next prefetches it as it scrolls into view
 * and on hover, and the product page is there when it's clicked. (Astryx's ClickableCard
 * hides its link in a 1px element, which the viewport prefetch never sees.) Each card is
 * also a view transition boundary: when the results change, cards that stay slide to
 * their place, new ones fade in and gone ones out (`.shop-card-*`, globals.css).
 */
export function ProductGrid({
  hits,
  digits,
  productBase,
  minWidth = 160,
  eager = 0,
  shared = false,
}: {
  hits: Hit[];
  digits: number;
  /** Product pages' URL prefix (`/hr/p`): a string, so client components can pass it. */
  productBase: string;
  minWidth?: number;
  /** How many of the first images load at once, with priority: the ones above the
   * fold on a page's first render (its largest paint); the rest load lazily. */
  eager?: number;
  /** Whether a card's image is the product's shared element: clicking it, the image
   * travels to the product page's hero (`productImageName`). Only where a product can
   * appear once on the page (a name may be mounted once), so not on the home page's
   * several shelves. */
  shared?: boolean;
}) {
  return (
    <Grid columns={{ minWidth, repeat: "fill" }} gap={5}>
      {hits.map((hit, i) => {
        const image =
          hit.image?.kind === "image" ? (hit.image.thumbnail_url ?? hit.image.url) : null;
        const first = i < eager;
        // The hit's live automatic discounts, once search sends them.
        const promotions = readPromotions(hit);
        return (
          <ViewTransition
            key={hit.listing}
            default="none"
            update="shop-card-move"
            enter="shop-card-in"
            exit="shop-card-out"
          >
            <Link
              href={`${productBase}/${hit.handle}`}
              className="shop-product-card"
              aria-label={hit.on_sale ? `${hit.title}, on sale` : hit.title}
            >
              <VStack gap={3}>
                <ViewTransition
                  name={shared ? productImageName(hit.handle) : undefined}
                  default="none"
                  share="shop-image-share"
                >
                  <div className="shop-rounded shop-product-image">
                    <AspectRatio ratio={4 / 5}>
                      {image ? (
                        // Plain <img>: the media URLs resize themselves.
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
                </ViewTransition>
                <VStack gap={1}>
                  {hit.vendor && (
                    <Text type="supporting" color="secondary">
                      {hit.vendor}
                    </Text>
                  )}
                  <Text type="body" weight="medium" maxLines={2}>
                    {hit.title}
                  </Text>
                  <Price
                    price={hit.price}
                    compareAt={hit.on_sale ? hit.compare_at : null}
                    text={formatRange(hit.listing_price.min, hit.listing_price.max, digits)}
                    digits={digits}
                    color="secondary"
                  />
                  {(!hit.available || hit.on_sale || promotions.length > 0) && (
                    <HStack gap={1} wrap="wrap">
                      {hit.on_sale && <SaleBadge />}
                      <PromotionBadges promotions={promotions} />
                      {!hit.available && <Badge label="Sold out" variant="neutral" />}
                    </HStack>
                  )}
                </VStack>
              </VStack>
            </Link>
          </ViewTransition>
        );
      })}
    </Grid>
  );
}
