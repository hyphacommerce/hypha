import { directionOf } from "@hyphacommerce/i18n";
import type { Metadata } from "next";

import "../globals.css";
import { LocaleProvider } from "@/components/locale";
import { ClientMessages } from "@/components/messages";
import { loadStore, pageLocale } from "@/lib/api";
import { astryxMessages } from "@/lib/astryx-messages";
import { pageT } from "@/lib/i18n";
import { SHARED_NAMESPACES } from "@/lib/messages";
import { link, siteUrl } from "@/lib/site";

import { Providers } from "../providers";

/** Pages under a country are static and regenerated in the background: none is built ahead,
 * since there's no API to read at build time; each is rendered on its first visit and kept,
 * then refreshed at most every five minutes. */
export const revalidate = 300;
export const dynamicParams = true;
export function generateStaticParams() {
  return [];
}

type Props = { children: React.ReactNode; params: Promise<{ country: string }> };

export async function generateMetadata({
  params,
}: {
  params: Promise<{ country: string }>;
}): Promise<Metadata> {
  const { country, language } = await pageLocale((await params).country);
  const store = await loadStore(country, language);
  return {
    // What relative canonical and preview URLs resolve against.
    metadataBase: siteUrl(),
    title: { default: store.name, template: `%s · ${store.name}` },
    description: store.description ?? undefined,
    applicationName: store.name,
    // The shop's icons in public/: replace them with the store's own.
    icons: {
      icon: [
        { url: "/favicon.ico", sizes: "32x32" },
        { url: "/favicon.svg", type: "image/svg+xml" },
      ],
      apple: "/apple-touch-icon.png",
    },
  };
}

/**
 * The document of a place (a country, in one of its languages): its `lang` and `dir`
 * follow the URL's locale segment, so it's the root layout, under the segment. It holds
 * what every page and its not-found page need: the theme, the page's language for
 * Astryx and for the client components (`ClientMessages`: the namespaces every page
 * reads) and the place for server actions (`LocaleProvider`). The shop around a page is
 * `(shop)/layout.tsx`. Nothing here reads the request, so the pages under it can be
 * static.
 *
 * Routes outside a place (the order link, the webhook, the sitemap) are route handlers,
 * and a path no route matches gets `app/global-not-found.tsx`. A segment naming a
 * country nobody sells to renders here in the store's language, and `(shop)` answers 404.
 */
export default async function PlaceLayout({ children, params }: Props) {
  const { country: segment } = await params;
  const [place, t] = await Promise.all([pageLocale(segment), pageT(segment)]);
  // The words' language, `<html lang>` and Astryx's: what the page is written in
  // (`lib/i18n.ts` says which that is for a language without a catalog).
  const language = t.language;
  const direction = directionOf(language);
  return (
    // `data-astryx-theme` on <html> from the server: the theme's tokens are scoped to it,
    // and Astryx's Theme only sets it there once hydrated, so until then the body showed
    // the default theme's background.
    <html lang={language} dir={direction} data-astryx-theme="shop" data-theme="light">
      <head>
        {/* The Latin typeface (globals.css) fetched with the page, not once the CSS
          has been parsed: less time in the fallback font. */}
        <link
          rel="preload"
          href="/fonts/geist-latin-wght-normal.woff2"
          as="font"
          type="font/woff2"
          crossOrigin="anonymous"
        />
      </head>
      <body>
        <Providers
          language={language}
          direction={direction}
          astryxMessages={await astryxMessages(language)}
        >
          <LocaleProvider segment={place.segment} home={link(place.segment, "/")}>
            <ClientMessages t={t} namespaces={SHARED_NAMESPACES}>
              {children}
            </ClientMessages>
          </LocaleProvider>
        </Providers>
      </body>
    </html>
  );
}
