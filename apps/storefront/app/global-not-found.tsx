// A path no page matches, or a country nobody sells to: there's no place, so no root
// layout (that's `app/[country]/layout.tsx`, whose document follows the place's
// language). This page is its own document, in the store's language.

import { Button } from "@astryxdesign/core/Button";
import { EmptyState } from "@astryxdesign/core/EmptyState";
import { Section } from "@astryxdesign/core/Section";
import { directionOf } from "@hyphacommerce/i18n";
import type { Metadata } from "next";

import "./globals.css";
import { astryxMessages } from "@/lib/astryx-messages";
import { getT, storeT } from "@/lib/i18n";

import { Providers } from "./providers";

export const metadata: Metadata = { title: "404" };

/** The store's translator, or English when the API can't be read (this page is
 * prerendered at build time, without one). */
async function translator() {
  try {
    return await storeT();
  } catch {
    return getT("en");
  }
}

export default async function GlobalNotFound() {
  const t = await translator();
  const direction = directionOf(t.language);
  return (
    <html lang={t.language} dir={direction} data-astryx-theme="shop" data-theme="light">
      <body>
        <Providers
          language={t.language}
          direction={direction}
          astryxMessages={await astryxMessages(t.language)}
        >
          <Section padding={10}>
            <EmptyState
              headingLevel={1}
              title={t("common.not_found_title")}
              description={t("common.not_found_description")}
              actions={<Button label={t("common.back_to_shop")} href="/" variant="primary" />}
            />
          </Section>
        </Providers>
      </body>
    </html>
  );
}
