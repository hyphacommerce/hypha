// Gives the client components under it their translator. The country layout sends the
// namespaces every page reads; a route whose forms read more (checkout, the account)
// renders another around its pages with those, added to what's above it, so no page is
// sent the catalog whole.

import type { Namespace, T } from "@/lib/messages";
import { clientMessages } from "@/lib/i18n";

import { MessagesProvider } from "./messages-provider";

export async function ClientMessages({
  t,
  namespaces,
  children,
}: {
  /** The page's translator: its language and locale are the browser's too. */
  t: T;
  namespaces: readonly Namespace[];
  children: React.ReactNode;
}) {
  return (
    <MessagesProvider
      language={t.language}
      locale={t.locale}
      messages={await clientMessages(t, namespaces)}
    >
      {children}
    </MessagesProvider>
  );
}
