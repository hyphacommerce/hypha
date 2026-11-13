"use client";

// The browser's side of `ClientMessages`: the messages it's given, added to those a
// provider above it was given, as one catalog for `useT`.

import type { Messages, Missing } from "@hyphacommerce/i18n";
import { I18nProvider } from "@hyphacommerce/i18n/react";
import { createContext, useContext, useMemo } from "react";

const Sent = createContext<Messages>({});

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
  const above = useContext(Sent);
  // By content, not identity: every server render of the layout (a navigation, an
  // action's `refresh()`) sends an equal catalog as a new object, and a new translator
  // would compile every message again and re-render everything that reads one.
  const sent = JSON.stringify(messages);
  const all = useMemo<Messages>(() => ({ ...above, ...JSON.parse(sent) }), [above, sent]);
  return (
    <Sent value={all}>
      <I18nProvider language={language} locale={locale} messages={all} onMissing={onMissing}>
        {children}
      </I18nProvider>
    </Sent>
  );
}
