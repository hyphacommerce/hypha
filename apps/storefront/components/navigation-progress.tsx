"use client";

// A thin bar along the top of the page while a navigation is under way (design note
// 0024, §7): a product page that isn't cached yet takes a moment to render, and without
// this a click looked like nothing happened. It starts when a link is followed
// (`SiteLink`'s `onNavigate`, or `useNavigationProgress().start()` before a
// `router.push` or an action that redirects) and ends when the page's path or query
// changes. A prefetched page arrives at once: the bar only appears after a short delay
// (CSS), so instant navigations never flash it.

import { usePathname, useSearchParams } from "next/navigation";
import {
  Suspense,
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";

type Progress = {
  /** Shows the bar until the next page change (or `done`). */
  start: () => void;
  /** Hides the bar: a navigation that didn't happen (an error, a refusal). */
  done: () => void;
};

const Context = createContext<Progress>({ start: () => {}, done: () => {} });

/** If nothing changed by then, the navigation failed or was cancelled: hide the bar. */
const GIVE_UP_MS = 15_000;

/** Ends the bar when the page changes: reads the query, so inside its own Suspense
 * boundary (a static page would otherwise render in the browser only). */
function Ender({ onNavigate }: { onNavigate: () => void }) {
  const pathname = usePathname();
  const search = useSearchParams().toString();
  const where = `${pathname}?${search}`;
  const last = useRef(where);
  useEffect(() => {
    if (last.current === where) return;
    last.current = where;
    onNavigate();
  }, [where, onNavigate]);
  return null;
}

export function NavigationProgress({ children }: { children: React.ReactNode }) {
  // A number rather than a flag: each start restarts the bar's animation (the key).
  const [run, setRun] = useState(0);
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const done = useCallback(() => {
    clearTimeout(timer.current);
    setRun(0);
  }, []);
  const start = useCallback(() => {
    clearTimeout(timer.current);
    timer.current = setTimeout(done, GIVE_UP_MS);
    setRun((n) => n + 1);
  }, [done]);
  useEffect(() => () => clearTimeout(timer.current), []);
  const value = useMemo(() => ({ start, done }), [start, done]);
  return (
    <Context value={value}>
      <Suspense fallback={null}>
        <Ender onNavigate={done} />
      </Suspense>
      {run > 0 && <div key={run} className="shop-progress" aria-hidden="true" />}
      {children}
    </Context>
  );
}

export function useNavigationProgress(): Progress {
  return useContext(Context);
}
