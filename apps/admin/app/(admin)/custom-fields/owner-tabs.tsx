"use client";

import { Tab, TabList } from "@astryxdesign/core/TabList";
import { useRouter } from "next/navigation";
import { type ReactNode, useTransition } from "react";

import { OWNER_TYPES, type OwnerType } from "@/lib/custom-fields";

/**
 * The owner types as tabs, above the chosen one's fields. Another owner type is
 * another URL (`?owner=`), read on the server: switching replaces the URL in a
 * transition, like a table's filters, so the tabs stay and the fields dim until the new
 * ones are there, and Back leaves the page rather than the tab.
 */
export function OwnerTabs({ owner, children }: { owner: OwnerType; children: ReactNode }) {
  const router = useRouter();
  const [isSwitching, startSwitch] = useTransition();
  return (
    <>
      <TabList
        value={owner}
        onChange={(value) =>
          startSwitch(() => router.replace(`/custom-fields?owner=${value}`, { scroll: false }))
        }
      >
        {OWNER_TYPES.map((o) => (
          <Tab key={o.value} value={o.value} label={o.label} />
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
