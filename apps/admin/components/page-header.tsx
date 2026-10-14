import { Breadcrumbs, BreadcrumbItem } from "@astryxdesign/core/Breadcrumbs";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Text } from "@astryxdesign/core/Text";
import type { ReactNode } from "react";

/** A page's title, with the way back above it and its actions on the right. */
export function PageHeader({
  title,
  description,
  back,
  badges,
  actions,
}: {
  title: string;
  description?: string;
  /** The parent page, shown as a breadcrumb. */
  back?: { label: string; href: string };
  badges?: ReactNode;
  actions?: ReactNode;
}) {
  return (
    <VStack gap={2}>
      {back && (
        <Breadcrumbs variant="supporting">
          <BreadcrumbItem href={back.href}>{back.label}</BreadcrumbItem>
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
