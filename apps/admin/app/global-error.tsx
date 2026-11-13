"use client";

// The last error boundary, for a failure in the root layout itself: it renders its own
// document, without the layout's providers. Code from an earlier deploy reloads the page
// once (`lib/stale-build.ts`).

import type { Messages } from "@hyphacommerce/i18n";
import { directionOf } from "@hyphacommerce/i18n";
import { useEffect, useState } from "react";

import { loadCatalog } from "@/lib/catalogs";
import { DEFAULT_LANGUAGE } from "@/lib/pick-language";
import { isStaleBuild, reloadForNewBuild } from "@/lib/stale-build";
// Its three sentences in English come with the page, so they show even when no other
// code can be loaded. They take no placeholders: they're shown as they are.
import english from "@/messages/en/global_error.json";

export default function GlobalError({ error, reset }: { error: Error; reset: () => void }) {
  // The document that failed already said which language it was in.
  const [language] = useState(() =>
    typeof document === "undefined"
      ? DEFAULT_LANGUAGE
      : document.documentElement.lang || DEFAULT_LANGUAGE,
  );
  const [catalog, setCatalog] = useState<Messages>();

  useEffect(() => {
    if (!(isStaleBuild(error) && reloadForNewBuild())) console.error(error);
  }, [error]);

  useEffect(() => {
    if (language === DEFAULT_LANGUAGE) return;
    let isCurrent = true;
    // Without the language's catalog (its chunk may be what failed), English stays.
    const load = async () => {
      try {
        const messages = await loadCatalog(language);
        if (isCurrent) setCatalog(messages);
      } catch {
        // English stays.
      }
    };
    void load();
    return () => {
      isCurrent = false;
    };
  }, [language]);

  const text = (key: keyof typeof english) => catalog?.[`global_error.${key}`] ?? english[key];

  return (
    <html lang={language} dir={directionOf(language)}>
      <body style={{ fontFamily: "system-ui, sans-serif", padding: 32 }}>
        <h1 style={{ fontSize: 20 }}>{text("title")}</h1>
        <p>{text("description")}</p>
        <button type="button" onClick={reset}>
          {text("retry")}
        </button>
      </body>
    </html>
  );
}
