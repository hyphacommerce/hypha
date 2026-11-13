"use client";

// The browser's side of `ClientMessages`: the messages it's given, added to those a
// provider above it was given, as one catalog for `useT`.

import type { Messages } from "@hyphacommerce/i18n";
import { I18nProvider } from "@hyphacommerce/i18n/react";
import { createContext, useContext, useMemo } from "react";

const Sent = createContext<Messages>({});

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
      <I18nProvider language={language} locale={locale} messages={all}>
        {children}
      </I18nProvider>
    </Sent>
  );
}
