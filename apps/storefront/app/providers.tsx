"use client";

import { LinkProvider } from "@astryxdesign/core/Link";
import { Theme } from "@astryxdesign/core/theme";

import { NavigationProgress } from "@/components/navigation-progress";
import { SiteLink } from "@/components/site-link";
import { shopTheme } from "@/theme/shop";

/** The shop's theme (`theme/shopTheme.ts`), the navigation bar, and Next.js links (with
 * the bar) for every Astryx link, button and card with an `href`. */
export function Providers({ children }: { children: React.ReactNode }) {
  return (
    <Theme theme={shopTheme}>
      <NavigationProgress>
        <LinkProvider component={SiteLink}>{children}</LinkProvider>
      </NavigationProgress>
    </Theme>
  );
}
