"use client";

import { Tab, TabList } from "@astryxdesign/core/TabList";
import { useRouter } from "next/navigation";
import { type ReactNode, useTransition } from "react";

/**
 * Tabs whose choice is in the URL, read on the server. Switching replaces the URL in a
 * transition, like a table's filters, so the tabs stay and the content dims until the
 * other tab's is there, and Back leaves the page rather than the tab.
 */
export function UrlTabs<T extends string>({
  tabs,
  value,
  href,
  children,
}: {
  tabs: readonly { value: T; label: string }[];
  value: T;
  /** The URL showing a tab. */
  href: (tab: T) => string;
  children: ReactNode;
}) {
  const router = useRouter();
  const [isSwitching, startSwitch] = useTransition();
  return (
    <>
      <TabList
        value={value}
        onChange={(next) => {
          const tab = tabs.find((t) => t.value === next);
          if (tab) startSwitch(() => router.replace(href(tab.value), { scroll: false }));
        }}
      >
        {tabs.map((tab) => (
          <Tab key={tab.value} value={tab.value} label={tab.label} />
        ))}
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
