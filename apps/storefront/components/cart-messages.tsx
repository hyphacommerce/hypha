// A cart's messages shown where they belong: errors (which stop checkout) in red,
// warnings in amber, the built-in ones in the page's language (`wordMessage`).

import { Banner } from "@astryxdesign/core/Banner";
import { VStack } from "@astryxdesign/core/Stack";

import { type CartMessage, type LabelOf, tone, wordMessage } from "@/lib/cart-messages";
import type { TFor } from "@/lib/messages";

export function CartMessages({
  t,
  messages,
  about,
  labelOf,
}: {
  /** The page's translator: a server page passes its own, a client component `useT`'s. */
  t: TFor<"cart">;
  messages: readonly CartMessage[] | undefined;
  /** What the messages are about, when the place they're shown doesn't say (a line). */
  about?: string;
  /** The address form's labels, for a message naming one of its fields. */
  labelOf?: LabelOf;
}) {
  if (!messages || messages.length === 0) return null;
  return (
    <VStack gap={2}>
      {messages.map((m) => {
        const text = wordMessage(t, m, labelOf);
        return (
          <Banner
            key={`${m.path} ${m.code} ${m.message}`}
            status={tone(m)}
            title={about ? `${about}: ${text}` : text}
          />
        );
      })}
    </VStack>
  );
}
