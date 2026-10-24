"use client";

import type { ReactNode } from "react";

import { UrlTabs } from "@/components/url-tabs";

export type DiscountTab = "settings" | "codes" | "usage";

const TABS = [
  { value: "settings", label: "Settings" },
  { value: "codes", label: "Codes" },
  { value: "usage", label: "Usage" },
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
  return (
    <UrlTabs
      tabs={TABS.filter(
        (t) => (t.value !== "codes" || hasCodes) && (t.value !== "usage" || hasUsage),
      )}
      value={tab}
      href={(t) => (t === "settings" ? `/discounts/${id}` : `/discounts/${id}?tab=${t}`)}
    >
      {children}
    </UrlTabs>
  );
}
