import type { Metadata } from "next";

import "./globals.css";
import { loadStore } from "@/lib/api";

import { Providers } from "./providers";

/**
 * The store's name and language, or defaults when the API can't be read: at build time
 * (nothing is prerendered then, but the not-found page is) or during an outage, a page
 * still renders. Pages under a country are static with ISR (design note 0024, §7);
 * search, cart, checkout and account render per request.
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
  // The language the store's content is written in (design note 0021, SU5).
  const store = await storeOrDefaults();
  return (
    <html lang={store.default_language}>
      <body>
        <Providers>{children}</Providers>
      </body>
    </html>
  );
}
