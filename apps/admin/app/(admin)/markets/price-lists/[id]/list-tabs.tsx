"use client";

import type { ReactNode } from "react";

import { UrlTabs } from "@/components/url-tabs";
import { useT } from "@/lib/use-t";

export type ListTab = "settings" | "prices";

/** A list's settings and its prices as tabs, the tab in the URL (`?tab=prices`). */
export function ListTabs({ id, tab, children }: { id: string; tab: ListTab; children: ReactNode }) {
  const t = useT<"price_lists">();
  const tabs: { value: ListTab; label: string }[] = [
    { value: "settings", label: t("common.settings") },
    { value: "prices", label: t("price_lists.detail_tab_prices") },
  ];
  return (
    <UrlTabs
      tabs={tabs}
      value={tab}
      href={(to) =>
        to === "prices" ? `/markets/price-lists/${id}?tab=prices` : `/markets/price-lists/${id}`
      }
    >
      {children}
    </UrlTabs>
  );
}
