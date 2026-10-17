"use client";

// Overlays close when the shopper goes somewhere else (design note 0024, §7): the cart
// drawer, the search popover, the filters sheet. Static pages keep the header and the
// layout mounted across navigations, so an open overlay would otherwise stay open on
// the next page.

import { usePathname, useSearchParams } from "next/navigation";
import { Suspense, useEffect, useRef } from "react";

function Watch({ onNavigate, pathOnly }: { onNavigate: () => void; pathOnly: boolean }) {
  const pathname = usePathname();
  const search = useSearchParams().toString();
  const where = pathOnly ? pathname : `${pathname}?${search}`;
  const last = useRef(where);
  const callback = useRef(onNavigate);
  callback.current = onNavigate;
  useEffect(() => {
    if (last.current === where) return;
    last.current = where;
    callback.current();
  }, [where]);
  return null;
}

/**
 * Calls `onNavigate` when the page changes: its path, and its query too unless
 * `pathOnly` (the filters sheet changes the query itself, and stays open while it
 * does). It renders nothing, inside its own Suspense boundary: reading the query on a
 * static page would otherwise make everything around it render in the browser only.
 */
export function CloseOnNavigate({
  onNavigate,
  pathOnly = false,
}: {
  onNavigate: () => void;
  pathOnly?: boolean;
}) {
  return (
    <Suspense fallback={null}>
      <Watch onNavigate={onNavigate} pathOnly={pathOnly} />
    </Suspense>
  );
}
