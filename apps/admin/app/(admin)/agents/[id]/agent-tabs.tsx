"use client";

import type { ReactNode } from "react";

import { UrlTabs } from "@/components/url-tabs";

export type AgentTab = "access" | "activity";

const TABS = [
  { value: "access", label: "Access" },
  { value: "activity", label: "Activity" },
] as const;

/** An agent's access and its activity as tabs, the tab in the URL (`?tab=activity`). */
export function AgentTabs({
  id,
  tab,
  children,
}: {
  id: string;
  tab: AgentTab;
  children: ReactNode;
}) {
  return (
    <UrlTabs
      tabs={TABS}
      value={tab}
      href={(t) => (t === "activity" ? `/agents/${id}?tab=activity` : `/agents/${id}`)}
    >
      {children}
    </UrlTabs>
  );
}
