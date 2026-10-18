// Loading states shaped like the pages they stand in for (design note 0024, §7), for the
// dynamic routes only (search, cart): their `loading.tsx` shows one while the page
// renders. Static pages have none: their HTML is the page, and a fallback there would be
// cached with it and flash before every visit (design note 0025, §6).

import { AspectRatio } from "@astryxdesign/core/AspectRatio";
import { Card } from "@astryxdesign/core/Card";
import { Grid } from "@astryxdesign/core/Grid";
import { HStack, StackItem, VStack } from "@astryxdesign/core/Stack";
import type { CSSProperties } from "react";

/**
 * A placeholder with Astryx's Skeleton look (its colour token, fade, stagger and the
 * reduced-motion and forced-colours fallbacks), as one element with one class
 * (`.shop-bone`, globals.css). Astryx's own carries some fifteen atomic classes and
 * inline variables, and a page's skeleton has a hundred placeholders, rendered with
 * every dynamic page even when its content is ready at once: a quarter of the search
 * page's HTML (design note 0025, §6).
 */
export function Skeleton({
  width = "100%",
  height = "100%",
  radius = 3,
  index = 0,
}: {
  width?: number | string;
  height?: number | string;
  /** Astryx's steps: 1 inner, 2 element, 3 container. */
  radius?: 1 | 2 | 3;
  /** Its place in a sequence: each starts fading 100 ms after the one before. */
  index?: number;
}) {
  const style = { width, height, "--bone-delay": `${1000 + 100 * index}ms` } as CSSProperties;
  return <div className="shop-bone" data-radius={radius} aria-hidden="true" style={style} />;
}

/** A product card: the image, the title, the price. */
function CardSkeleton({ index }: { index: number }) {
  return (
    <VStack gap={3}>
      <AspectRatio ratio={4 / 5}>
        <Skeleton index={index} radius={3} />
      </AspectRatio>
      <VStack gap={2}>
        <Skeleton index={index} height={16} width="80%" />
        <Skeleton index={index} height={14} width="40%" />
      </VStack>
    </VStack>
  );
}

/** A grid of `count` product cards, as `ProductGrid` lays them out. */
export function ProductGridSkeleton({
  count = 8,
  minWidth = 180,
}: {
  count?: number;
  minWidth?: number;
}) {
  return (
    <Grid columns={{ minWidth, repeat: "fill" }} gap={5}>
      {Array.from({ length: count }, (_, i) => (
        <CardSkeleton key={i} index={i} />
      ))}
    </Grid>
  );
}

/** A facet: its name and a few values. */
function FacetSkeleton({ index }: { index: number }) {
  return (
    <VStack gap={2}>
      <Skeleton index={index} height={14} width={80} />
      {[0, 1, 2].map((i) => (
        <HStack key={i} gap={2} vAlign="center">
          <Skeleton index={index} height={16} width={16} radius={1} />
          <Skeleton index={index} height={14} width={`${60 - i * 10}%`} />
        </HStack>
      ))}
    </VStack>
  );
}

/** A heading's line. */
function TitleSkeleton({ width = 280 }: { width?: number }) {
  return <Skeleton height={44} width={width} />;
}

/** A category or search page: the title, the facets beside the results. */
export function ListingPageSkeleton() {
  return (
    <VStack gap={6}>
      <VStack gap={3}>
        <Skeleton height={14} width={140} />
        <TitleSkeleton />
      </VStack>
      <HStack gap={8} vAlign="start">
        <VStack width={240} gap={6}>
          {[0, 1, 2].map((i) => (
            <FacetSkeleton key={i} index={i} />
          ))}
        </VStack>
        <StackItem size="fill">
          <VStack gap={5}>
            <HStack hAlign="between">
              <Skeleton height={14} width={90} />
              <Skeleton height={32} width={120} />
            </HStack>
            <ProductGridSkeleton count={9} />
          </VStack>
        </StackItem>
      </HStack>
    </VStack>
  );
}

/** The cart page: lines beside the summary. */
export function CartPageSkeleton() {
  return (
    <VStack gap={6}>
      <TitleSkeleton width={140} />
      <HStack gap={6} vAlign="start" wrap="wrap">
        <StackItem size="fill">
          <Card padding={5}>
            <VStack gap={4}>
              {[0, 1, 2].map((i) => (
                <HStack key={i} gap={3} vAlign="start">
                  <Skeleton index={i} height={80} width={80} />
                  <StackItem size="fill">
                    <VStack gap={2}>
                      <Skeleton index={i} height={16} width="50%" />
                      <Skeleton index={i} height={14} width="30%" />
                      <Skeleton index={i} height={32} width={104} />
                    </VStack>
                  </StackItem>
                </HStack>
              ))}
            </VStack>
          </Card>
        </StackItem>
        <VStack width={360}>
          <Card padding={5}>
            <VStack gap={3}>
              <Skeleton height={24} width={120} />
              {[0, 1, 2, 3].map((i) => (
                <Skeleton key={i} index={i} height={14} />
              ))}
              <Skeleton height={44} />
            </VStack>
          </Card>
        </VStack>
      </HStack>
    </VStack>
  );
}
