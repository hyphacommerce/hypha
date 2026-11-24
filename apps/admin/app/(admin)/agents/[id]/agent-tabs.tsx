"use client";

import { type ReactNode, useMemo } from "react";

import { UrlTabs } from "@/components/url-tabs";
import { useT } from "@/lib/use-t";

export type AgentTab = "access" | "activity";

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
  const t = useT<"agents">();
  const tabs = useMemo(
    () =>
      [
        { value: "access", label: t("agents.tab_access") },
        { value: "activity", label: t("agents.tab_activity") },
      ] as const,
    [t],
  );
  return (
    <UrlTabs
      tabs={tabs}
      value={tab}
      href={(next) => (next === "activity" ? `/agents/${id}?tab=activity` : `/agents/${id}`)}
    >
      {children}
    </UrlTabs>
  );
}
