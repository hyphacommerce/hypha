"use client";

import { LinkProvider } from "@astryxdesign/core/Link";
import { Theme } from "@astryxdesign/core/theme";
import { hyphaTheme } from "@/theme/hypha";
import Link from "next/link";
import { useEffect } from "react";

import { reloadForNewBuild } from "@/lib/stale-build";

/** The Hypha theme (`theme/hyphaTheme.ts`), Next.js links for every Astryx link and nav
 * item, and a reload onto the new build when an earlier deploy's code can't load. */
export function Providers({ children }: { children: React.ReactNode }) {
  // Vite's loader says when code split out of the build couldn't be loaded (an earlier
  // deploy's): load the page again, onto the new build (`lib/stale-build.ts`).
  useEffect(() => {
    const onPreloadError = (event: Event) => {
      if (reloadForNewBuild()) event.preventDefault();
    };
    window.addEventListener("vite:preloadError", onPreloadError);
    return () => window.removeEventListener("vite:preloadError", onPreloadError);
  }, []);
  return (
    <Theme theme={hyphaTheme}>
      <LinkProvider component={Link}>{children}</LinkProvider>
    </Theme>
  );
}
