// Loading states. A page renders what it knows without the API at once (its title,
// description, breadcrumbs, actions, tabs, a table's toolbar) and streams the parts
// that need data behind `Suspense`, each with a skeleton of its own shape from here.
// Nothing above a page suspends: the admin has no `loading.tsx`, since one would wrap
// the whole page, static parts included, and nest a second boundary around the page's
// own.

import { Card } from "@astryxdesign/core/Card";
import { VStack } from "@astryxdesign/core/Layout";
import { Skeleton } from "@astryxdesign/core/Skeleton";

import { type Crumb, PageHeader } from "./page-header";

/** A table's rows: the header line and `rows` lines, inside a table's card. */
export function RowsSkeleton({ rows = 8 }: { rows?: number }) {
  return (
    <VStack gap={3}>
      {Array.from({ length: rows + 1 }, (_, i) => (
        <Skeleton key={i} index={i} height={32} />
      ))}
    </VStack>
  );
}

/** A whole table card: the toolbar, the header and `rows` lines. */
export function TableSkeleton({ rows = 8 }: { rows?: number }) {
  return (
    <div className="table-skeleton">
      <Skeleton height={36} />
      <RowsSkeleton rows={rows} />
    </div>
  );
}

/** A card of `lines` text lines under a heading. */
export function CardSkeleton({ lines = 4 }: { lines?: number }) {
  return (
    <Card>
      <VStack gap={3}>
        <Skeleton width={140} height={24} />
        {Array.from({ length: lines }, (_, i) => (
          <Skeleton key={i} index={i} height={16} width={i % 2 ? "70%" : "100%"} />
        ))}
      </VStack>
    </Card>
  );
}

/** A form or settings body: a column of `cards` cards. */
export function FormSkeleton({ cards = 2 }: { cards?: number }) {
  return (
    <VStack gap={5}>
      {Array.from({ length: cards }, (_, i) => (
        <CardSkeleton key={i} lines={i === 0 ? 5 : 4} />
      ))}
    </VStack>
  );
}

/**
 * A record's page while the record loads: its header with the way `back` and a line
 * for the title, then the main cards beside the side ones.
 */
export function DetailSkeleton({ back }: { back?: Crumb | Crumb[] }) {
  return (
    <VStack gap={5}>
      <PageHeader back={back} />
      <div className="detail-columns">
        <div>
          <CardSkeleton lines={6} />
          <CardSkeleton lines={4} />
        </div>
        <div>
          <CardSkeleton lines={3} />
          <CardSkeleton lines={3} />
        </div>
      </div>
    </VStack>
  );
}
