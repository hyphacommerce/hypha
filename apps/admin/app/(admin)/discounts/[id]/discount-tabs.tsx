"use client";

import type { ReactNode } from "react";

import { UrlTabs } from "@/components/url-tabs";

export type DiscountTab = "settings" | "codes" | "usage";

const TABS = [
  { value: "settings", label: "Settings" },
  { value: "codes", label: "Codes" },
  { value: "usage", label: "Usage" },
] as const;

/** A discount's settings, codes and usage as tabs, the tab in the URL (`?tab=codes`). */
export function DiscountTabs({
  id,
  tab,
  children,
}: {
  id: string;
  tab: DiscountTab;
  children: ReactNode;
}) {
  return (
    <UrlTabs
      tabs={TABS}
      value={tab}
      href={(t) => (t === "settings" ? `/discounts/${id}` : `/discounts/${id}?tab=${t}`)}
    >
      {children}
    </UrlTabs>
  );
}
