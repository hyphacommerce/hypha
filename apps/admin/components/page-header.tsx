import { Breadcrumbs, BreadcrumbItem } from "@astryxdesign/core/Breadcrumbs";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Text } from "@astryxdesign/core/Text";
import type { ReactNode } from "react";

/** A page above this one, shown as a breadcrumb. */
export type Crumb = { label: string; href: string };

/** A page's title, with the way back above it and its actions on the right. */
export function PageHeader({
  title,
  description,
  back,
  badges,
  actions,
}: {
  title: string;
  description?: ReactNode;
  /** The parent page, or the pages above this one from the top, shown as breadcrumbs. */
  back?: Crumb | Crumb[];
  badges?: ReactNode;
  actions?: ReactNode;
}) {
  const trail = back === undefined ? [] : Array.isArray(back) ? back : [back];
  return (
    <VStack gap={2}>
      {trail.length > 0 && (
        <Breadcrumbs variant="supporting">
          {trail.map((crumb) => (
            <BreadcrumbItem key={crumb.href} href={crumb.href}>
              {crumb.label}
            </BreadcrumbItem>
          ))}
          <BreadcrumbItem>{title}</BreadcrumbItem>
        </Breadcrumbs>
      )}
      <HStack gap={4} vAlign="center" hAlign="between" wrap="wrap">
        <HStack gap={3} vAlign="center" wrap="wrap">
          <Text type="display-1" as="h1">
            {title}
          </Text>
          {badges}
        </HStack>
        {actions && (
          <HStack gap={2} vAlign="center">
            {actions}
          </HStack>
        )}
      </HStack>
      {description && (
        <Text type="body" color="secondary">
          {description}
        </Text>
      )}
    </VStack>
  );
}
