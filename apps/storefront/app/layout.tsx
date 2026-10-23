import type { Metadata } from "next";

import "./globals.css";
import { loadStore } from "@/lib/api";

import { Providers } from "./providers";

/**
 * The store's name and language, or defaults when the API can't be read: at build time
 * (nothing is prerendered then, but the not-found page is) or during an outage, a page
 * still renders. Pages under a country are static with ISR; search, cart, checkout and
 * account render per request.
 */
async function storeOrDefaults() {
  try {
    return await loadStore();
  } catch {
    return { name: "Store", description: null, default_language: "en" };
  }
}

export async function generateMetadata(): Promise<Metadata> {
  const store = await storeOrDefaults();
  return {
    title: { default: store.name, template: `%s · ${store.name}` },
    description: store.description ?? undefined,
  };
}

export default async function RootLayout({ children }: { children: React.ReactNode }) {
  // The language the store's content is written in.
  const store = await storeOrDefaults();
  return (
    // `data-astryx-theme` on <html> from the server: the theme's tokens are scoped to it,
    // and Astryx's Theme only sets it there once hydrated, so until then the body showed
    // the default theme's background.
    <html lang={store.default_language} data-astryx-theme="shop" data-theme="light">
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
        <Providers>{children}</Providers>
      </body>
    </html>
  );
}
