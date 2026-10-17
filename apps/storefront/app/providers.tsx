"use client";

import { LinkProvider } from "@astryxdesign/core/Link";
import { Theme } from "@astryxdesign/core/theme";
import Link from "next/link";

import { shopTheme } from "@/theme/shop";

/** The shop's theme (`theme/shopTheme.ts`), and Next.js links for every Astryx link,
 * button and card with an `href`. */
export function Providers({ children }: { children: React.ReactNode }) {
  return (
    <Theme theme={shopTheme}>
      <LinkProvider component={Link}>{children}</LinkProvider>
    </Theme>
  );
}
