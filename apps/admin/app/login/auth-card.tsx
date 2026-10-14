"use client";

// The split login card from Astryx's `login-split` template, with Hypha's content: the
// form on one side, a brand panel on the other, stacking on narrow screens.

import { Card } from "@astryxdesign/core/Card";
import { Center } from "@astryxdesign/core/Center";
import { Grid } from "@astryxdesign/core/Grid";
import { Icon } from "@astryxdesign/core/Icon";
import { HStack, StackItem, VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Text } from "@astryxdesign/core/Text";
import { SparklesIcon } from "@heroicons/react/24/outline";
import type { CSSProperties, ReactNode } from "react";

// Grid emits minmax(MIN, 1fr) where MIN is a hard floor: 320 − 2×24 (page) − 2×16
// (stacked inset) = 240, so the column fits the narrowest phone.
const COLUMN_MIN_WIDTH = 240;

const pageStyle: CSSProperties = {
  minHeight: "100%",
  backgroundColor: "var(--color-background-body)",
};
const cardWrap: CSSProperties = { width: "100%", maxWidth: 960, marginInline: "auto" };
const brandPanel: CSSProperties = {
  height: "100%",
  minHeight: 240,
  borderRadius: "var(--radius-container)",
  padding: "var(--spacing-8)",
  background:
    "linear-gradient(135deg, var(--color-background-green, #e7f5ec), var(--color-background-teal, #e3f4f4))",
};

// The container query is a plain <style> tag, so it needs no CSS compiler. The grid
// collapses to one column below 511px; the brand panel then moves above the form.
const AUTH_CARD_CSS = `
.auth-card-grid {
  container-type: inline-size;
  container-name: auth-card;
  padding: var(--spacing-8);
}
.auth-card-brand { width: 100%; order: 0; }
@container auth-card (max-width: 511px) {
  .auth-card-grid { padding: var(--spacing-4); }
  .auth-card-brand { order: -1; }
}
`;

export function AuthCard({
  title,
  subtitle,
  children,
}: {
  title: string;
  subtitle: string;
  children: ReactNode;
}) {
  return (
    <Center axis="both" padding={6} style={pageStyle}>
      <style>{AUTH_CARD_CSS}</style>
      <div style={cardWrap}>
        <Card padding={0} width="100%">
          <Grid
            columns={{ minWidth: COLUMN_MIN_WIDTH, repeat: "fit" }}
            gap={8}
            align="stretch"
            className="auth-card-grid"
          >
            <Section variant="transparent" padding={0} height="100%">
              <VStack gap={4} height="100%">
                <HStack gap={2} vAlign="center">
                  <Icon icon={SparklesIcon} />
                  <Text type="body" weight="bold">
                    Hypha
                  </Text>
                </HStack>
                <StackItem size="fill">
                  <Center axis="vertical" height="100%">
                    <VStack gap={4} hAlign="stretch" width="100%">
                      <VStack gap={1}>
                        <Text type="display-1" as="h1">
                          {title}
                        </Text>
                        <Text type="body" color="secondary" size="sm">
                          {subtitle}
                        </Text>
                      </VStack>
                      {children}
                    </VStack>
                  </Center>
                </StackItem>
              </VStack>
            </Section>
            <div className="auth-card-brand">
              <div style={brandPanel}>
                <VStack gap={2}>
                  <Text type="display-2" as="p">
                    Run your store.
                  </Text>
                  <Text type="body" color="secondary">
                    Products, markets, inventory and orders, in one place.
                  </Text>
                </VStack>
              </div>
            </div>
          </Grid>
        </Card>
      </div>
    </Center>
  );
}
