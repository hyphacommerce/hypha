"use client";

import type { ReactNode } from "react";

import { UrlTabs } from "@/components/url-tabs";

export type SupplierTab = "details" | "catalogue";

const TABS = [
  { value: "details", label: "Details" },
  { value: "catalogue", label: "Catalogue" },
] as const;

/** A supplier's details and catalogue as tabs, the tab in the URL (`?tab=catalogue`). */
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
