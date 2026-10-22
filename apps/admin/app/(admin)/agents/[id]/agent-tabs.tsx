"use client";

import { Tab, TabList } from "@astryxdesign/core/TabList";
import { useRouter } from "next/navigation";
import { type ReactNode, useTransition } from "react";

export type AgentTab = "access" | "activity";

/**
 * An agent's access and its activity as tabs. The tab is in the URL (`?tab=activity`),
 * read on the server: switching replaces the URL in a transition, so the tabs stay and
 * the content dims until the other tab's is there, and Back leaves the page.
 */
export function AgentTabs({
  id,
  tab,
  children,
}: {
  id: string;
  tab: AgentTab;
  children: ReactNode;
}) {
  const router = useRouter();
  const [isSwitching, startSwitch] = useTransition();
  return (
    <>
      <TabList
        value={tab}
        onChange={(value) =>
          startSwitch(() =>
            router.replace(value === "activity" ? `/agents/${id}?tab=activity` : `/agents/${id}`, {
              scroll: false,
            }),
          )
        }
      >
        <Tab value="access" label="Access" />
        <Tab value="activity" label="Activity" />
      </TabList>
      <div
        className="pending-content"
        aria-busy={isSwitching}
        data-pending={isSwitching || undefined}
      >
        {children}
      </div>
    </>
  );
}
