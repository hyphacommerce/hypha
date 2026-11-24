import { directionOf } from "@hyphacommerce/i18n";
import type { Metadata } from "next";

import { getT } from "@/lib/i18n";
import { getLanguage } from "@/lib/language";

import "./globals.css";
import { Providers } from "./providers";

export async function generateMetadata(): Promise<Metadata> {
  const t = await getT();
  return {
    title: t("nav.app_name"),
    // The brand's favicon set (public/), from the logo files.
    icons: {
      icon: [
        { url: "/favicon.ico", sizes: "32x32" },
        { url: "/favicon.svg", type: "image/svg+xml" },
      ],
      apple: "/apple-touch-icon.png",
    },
    manifest: "/site.webmanifest",
  };
}

/**
 * The document, in the staff member's language (`lib/language.ts`). The providers get
 * the language alone: each loads its catalog as a chunk the browser keeps, so no
 * navigation carries the catalog again.
 */
export default async function RootLayout({ children }: { children: React.ReactNode }) {
  const language = await getLanguage();
  return (
    <html lang={language} dir={directionOf(language)}>
      <body>
        <Providers language={language}>{children}</Providers>
      </body>
    </html>
  );
}
