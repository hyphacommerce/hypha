"use client";

import type { ReactNode } from "react";

import { UrlTabs } from "@/components/url-tabs";

export type SupplierTab = "details" | "catalogue" | "orders";

const TABS = [
  { value: "details", label: "Details" },
  { value: "catalogue", label: "Catalogue" },
  { value: "orders", label: "Purchase orders" },
] as const;

/**
 * A supplier's details, catalogue and purchase orders as tabs, the tab in the URL
 * (`?tab=catalogue`).
 */
export function SupplierTabs({
  id,
  tab,
  children,
}: {
  id: string;
  tab: SupplierTab;
  children: ReactNode;
}) {
  return (
    <UrlTabs
      tabs={[...TABS]}
      value={tab}
      href={(t) => (t === "details" ? `/suppliers/${id}` : `/suppliers/${id}?tab=${t}`)}
    >
      {children}
    </UrlTabs>
  );
}
