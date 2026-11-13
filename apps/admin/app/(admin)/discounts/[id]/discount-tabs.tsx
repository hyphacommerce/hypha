"use client";

import type { ReactNode } from "react";

import { UrlTabs } from "@/components/url-tabs";
import { useT } from "@/lib/use-t";

export type DiscountTab = "settings" | "codes" | "usage";

const TABS = [
  { value: "settings", label: "common.settings" },
  { value: "codes", label: "discounts.tab_codes" },
  { value: "usage", label: "discounts.tab_usage" },
] as const;

/**
 * A discount's settings, codes and usage as tabs, the tab in the URL (`?tab=codes`). An
 * automatic discount has no codes, so no codes tab; usage shows only to staff who may read
 * customers and orders.
 */
export function DiscountTabs({
  id,
  tab,
  hasCodes,
  hasUsage,
  children,
}: {
  id: string;
  tab: DiscountTab;
  hasCodes: boolean;
  hasUsage: boolean;
  children: ReactNode;
}) {
  const t = useT<"discounts">();
  return (
    <UrlTabs
      tabs={TABS.filter(
        ({ value }) => (value !== "codes" || hasCodes) && (value !== "usage" || hasUsage),
      ).map(({ value, label }) => ({ value, label: t(label) }))}
      value={tab}
      href={(to) => (to === "settings" ? `/discounts/${id}` : `/discounts/${id}?tab=${to}`)}
    >
      {children}
    </UrlTabs>
  );
}
