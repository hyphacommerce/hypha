"use client";

// The browser's side of `ClientMessages`: the package's provider (one inside another
// adds its messages to the outer one's), kept steady across server renders, with a
// warning in development for a namespace the page wasn't sent.

import type { Messages, Missing } from "@hyphacommerce/i18n";
import { I18nProvider } from "@hyphacommerce/i18n/react";

import { useSteadyCatalog } from "@/lib/use-steady-catalog";

/** A key no provider above the component was given: its namespace wasn't sent to this
 * page, and the key itself is on screen. */
function warn({ key }: Missing) {
  console.warn(
    `i18n: "${key}" was read by a client component, but its namespace wasn't sent to this page. ` +
      "Send it with <ClientMessages> in the route's layout, and ask for it with useT<…>().",
  );
}

/** Told only in development: in production a missing message just shows its key. */
const onMissing = process.env.NODE_ENV === "development" ? warn : undefined;

export function MessagesProvider({
  language,
  locale,
  messages,
  children,
}: {
  language: string;
  /** What numbers, money and dates are formatted for (`de-CH`). */
  locale: string;
  messages: Messages;
  children: React.ReactNode;
}) {
  // By content, not identity: the provider would otherwise make a new translator for
  // each server render's equal catalog, compiling every message again.
  const steady = useSteadyCatalog(messages);
  return (
    <I18nProvider language={language} locale={locale} messages={steady} onMissing={onMissing}>
      {children}
    </I18nProvider>
  );
}
