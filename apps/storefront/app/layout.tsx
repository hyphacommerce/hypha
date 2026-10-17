import type { Metadata } from "next";

import "./globals.css";
import { loadStore } from "@/lib/api";

// Every page renders per request, priced for the shopper's country (design note 0023,
// SF1): nothing is prerendered at build time, when there's no API to read.
export const dynamic = "force-dynamic";

export async function generateMetadata(): Promise<Metadata> {
  const store = await loadStore();
  return {
    title: { default: store.name, template: `%s · ${store.name}` },
    description: store.description ?? undefined,
  };
}

export default async function RootLayout({ children }: { children: React.ReactNode }) {
  // The language the store's content is written in (design note 0021, SU5).
  const store = await loadStore();
  return (
    <html lang={store.default_language}>
      <body>{children}</body>
    </html>
  );
}
