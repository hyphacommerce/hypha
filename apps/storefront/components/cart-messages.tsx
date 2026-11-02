// A cart's messages shown where they belong: errors (which stop checkout) in red,
// warnings in amber.

import { Banner } from "@astryxdesign/core/Banner";
import { VStack } from "@astryxdesign/core/Stack";

import { type CartMessage, tone } from "@/lib/messages";

export function CartMessages({
  messages,
  about,
}: {
  messages: readonly CartMessage[] | undefined;
  /** What the messages are about, when the place they're shown doesn't say (a line). */
  about?: string;
}) {
  if (!messages || messages.length === 0) return null;
  return (
    <VStack gap={2}>
      {messages.map((m) => (
        <Banner
          key={`${m.path} ${m.code} ${m.message}`}
          status={tone(m)}
          title={about ? `${about}: ${m.message}` : m.message}
        />
      ))}
    </VStack>
  );
}
