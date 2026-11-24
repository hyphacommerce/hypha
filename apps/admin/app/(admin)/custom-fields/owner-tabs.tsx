"use client";

import type { ReactNode } from "react";

import { UrlTabs } from "@/components/url-tabs";
import { OWNER_TYPES, type OwnerType } from "@/lib/custom-fields";
import { useT } from "@/lib/use-t";

/** The owner types as tabs above the chosen one's fields, the owner in the URL (`?owner=`). */
export function OwnerTabs({ owner, children }: { owner: OwnerType; children: ReactNode }) {
  const t = useT();
  const tabs = OWNER_TYPES.map((o) => ({ value: o.value, label: t(o.label) }));
  return (
    <UrlTabs tabs={tabs} value={owner} href={(o) => `/custom-fields?owner=${o}`}>
      {children}
    </UrlTabs>
  );
}
