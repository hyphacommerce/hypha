"use client";

import type { ReactNode } from "react";

import { UrlTabs } from "@/components/url-tabs";

export type ListTab = "settings" | "prices";

const TABS = [
  { value: "settings", label: "Settings" },
  { value: "prices", label: "Prices" },
] as const;

/** A list's settings and its prices as tabs, the tab in the URL (`?tab=prices`). */
export function ListTabs({ id, tab, children }: { id: string; tab: ListTab; children: ReactNode }) {
  return (
    <UrlTabs
      tabs={TABS}
      value={tab}
      href={(t) =>
        t === "prices" ? `/markets/price-lists/${id}?tab=prices` : `/markets/price-lists/${id}`
      }
    >
      {children}
    </UrlTabs>
  );
}
