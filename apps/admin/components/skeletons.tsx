// Loading states (design note 0024, section 6): every route renders the admin's frame at
// once and, while its data loads, a skeleton of its page with the title already in
// place. Each route's `loading.tsx` picks one of these.

import { Card } from "@astryxdesign/core/Card";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Skeleton } from "@astryxdesign/core/Skeleton";
import { Text } from "@astryxdesign/core/Text";

function Title({ title, back }: { title?: string; back?: string }) {
  return (
    <VStack gap={2}>
      {back !== undefined && <Skeleton width={120} height={14} />}
      {title ? (
        <Text type="display-1" as="h1">
          {title}
        </Text>
      ) : (
        <Skeleton width={260} height={36} />
      )}
    </VStack>
  );
}

/** A table's rows: the toolbar, the header and `rows` lines. */
export function TableSkeleton({ rows = 8 }: { rows?: number }) {
  return (
    <div className="table-skeleton">
      <Skeleton height={36} />
      <VStack gap={3}>
        {Array.from({ length: rows }, (_, i) => (
          <Skeleton key={i} index={i} height={32} />
        ))}
      </VStack>
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

/** A list page: its title (and its add button, when it has one), then a table. */
export function ListPageSkeleton({ title, hasAction = false }: { title: string; hasAction?: boolean }) {
  return (
    <Section padding={6}>
      <VStack gap={5}>
        <HStack hAlign="between" vAlign="center">
          <Title title={title} />
          {hasAction && <Skeleton width={120} height={36} />}
        </HStack>
        <TableSkeleton />
      </VStack>
    </Section>
  );
}

/** A detail page: breadcrumb and title, the main cards beside the side ones. */
export function DetailPageSkeleton({ title }: { title?: string }) {
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={1280}>
        <Title title={title} back="" />
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
    </Section>
  );
}

/** A settings or form page: its title and a column of cards. */
export function FormPageSkeleton({ title }: { title: string }) {
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={960}>
        <Title title={title} />
        <CardSkeleton lines={5} />
        <CardSkeleton lines={4} />
      </VStack>
    </Section>
  );
}
