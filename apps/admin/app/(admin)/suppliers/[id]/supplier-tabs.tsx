"use client";

import type { ReactNode } from "react";

import { UrlTabs } from "@/components/url-tabs";
import { useT } from "@/lib/use-t";

export type SupplierTab = "details" | "catalogue" | "orders";

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
  const t = useT<"suppliers">();
  const tabs: { value: SupplierTab; label: string }[] = [
    { value: "details", label: t("common.details") },
    { value: "catalogue", label: t("suppliers.tab_catalogue") },
    { value: "orders", label: t("suppliers.tab_orders") },
  ];
  return (
    <UrlTabs
      tabs={tabs}
      value={tab}
      href={(to) => (to === "details" ? `/suppliers/${id}` : `/suppliers/${id}?tab=${to}`)}
    >
      {children}
    </UrlTabs>
  );
}
