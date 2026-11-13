"use client";

import { InternationalizationProvider } from "@astryxdesign/core/i18n";
import { LinkProvider } from "@astryxdesign/core/Link";
import { Theme } from "@astryxdesign/core/theme";

import { NavigationProgress } from "@/components/navigation-progress";
import { SiteLink } from "@/components/site-link";
import { shopTheme } from "@/theme/shop";

/** The shop's theme (`theme/shopTheme.ts`), always light like hyphacommerce.com, the
 * navigation bar, Next.js links (with the bar) for every Astryx link, button and card
 * with an `href`, and the page's language for Astryx's components (their own words stay
 * English until their catalog for the language is passed here too). */
export function Providers({
  language,
  direction,
  children,
}: {
  language: string;
  /** The page's direction, as `<html dir>` has it: Astryx's own guess from the locale
   * may differ, and its components must agree with the document. */
  direction: "ltr" | "rtl";
  children: React.ReactNode;
}) {
  return (
    <InternationalizationProvider locale={language} dir={direction}>
      <Theme theme={shopTheme} mode="light">
        <NavigationProgress>
          <LinkProvider component={SiteLink}>{children}</LinkProvider>
        </NavigationProgress>
      </Theme>
    </InternationalizationProvider>
  );
}
