"use client";

import type { ReactNode } from "react";

import { UrlTabs } from "@/components/url-tabs";
import { OWNER_TYPES, type OwnerType } from "@/lib/custom-fields";

/** The owner types as tabs above the chosen one's fields, the owner in the URL (`?owner=`). */
export function OwnerTabs({ owner, children }: { owner: OwnerType; children: ReactNode }) {
  return (
    <UrlTabs tabs={OWNER_TYPES} value={owner} href={(o) => `/custom-fields?owner=${o}`}>
      {children}
    </UrlTabs>
  );
}
