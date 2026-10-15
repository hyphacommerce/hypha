"use client";

import { LinkProvider } from "@astryxdesign/core/Link";
import { Theme } from "@astryxdesign/core/theme";
import { hyphaTheme } from "@/theme/hypha";
import Link from "next/link";

/** The Hypha theme (`theme/hyphaTheme.ts`), and Next.js links for every Astryx link
 * and nav item. */
export function Providers({ children }: { children: React.ReactNode }) {
  return (
    <Theme theme={hyphaTheme}>
      <LinkProvider component={Link}>{children}</LinkProvider>
    </Theme>
  );
}
