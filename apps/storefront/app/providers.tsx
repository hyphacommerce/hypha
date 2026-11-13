"use client";

import { InternationalizationProvider } from "@astryxdesign/core/i18n";
import { LinkProvider } from "@astryxdesign/core/Link";
import { Theme } from "@astryxdesign/core/theme";
import { useMemo } from "react";

import { NavigationProgress } from "@/components/navigation-progress";
import { SiteLink } from "@/components/site-link";
import { shopTheme } from "@/theme/shop";

/** The shop's theme (`theme/shopTheme.ts`), always light like hyphacommerce.com, the
 * navigation bar, Next.js links (with the bar) for every Astryx link, button and card
 * with an `href`, and the page's language for Astryx's components, with their own words
 * in it (`astryxMessages`, from the server; English is built in). */
export function Providers({
  language,
  direction,
  astryxMessages,
  children,
}: {
  /** The language the page is written in: the storefront's translator's too. */
  language: string;
  /** The page's direction, as `<html dir>` has it: Astryx's own guess from the locale
   * may differ, and its components must agree with the document. */
  direction: "ltr" | "rtl";
  astryxMessages?: Readonly<Record<string, string>>;
  children: React.ReactNode;
}) {
  // By content: the layout sends an equal catalog as a new object on every server
  // render, and Astryx would build its lookup again for each.
  const sent = astryxMessages ? JSON.stringify(astryxMessages) : undefined;
  const messages = useMemo(
    () => (sent ? { [language]: JSON.parse(sent) } : undefined),
    [language, sent],
  );
  return (
    <InternationalizationProvider locale={language} dir={direction} messages={messages}>
      <Theme theme={shopTheme} mode="light">
        <NavigationProgress>
          <LinkProvider component={SiteLink}>{children}</LinkProvider>
        </NavigationProgress>
      </Theme>
    </InternationalizationProvider>
  );
}
