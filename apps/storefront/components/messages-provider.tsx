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
  const all = useMemo(() => ({ ...above, ...messages }), [above, messages]);
  return (
    <Sent value={all}>
      <I18nProvider language={language} locale={locale} messages={all}>
        {children}
      </I18nProvider>
    </Sent>
  );
}
