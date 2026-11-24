"use client";

// The worklist's two required choices, the language and the state: always one of each, so
// they sit above the table rather than among its filters (which can be cleared). A change
// replaces the URL in a transition and starts the list again; the table dims meanwhile.

import { HStack } from "@astryxdesign/core/Layout";
import { SegmentedControl, SegmentedControlItem } from "@astryxdesign/core/SegmentedControl";
import { Selector } from "@astryxdesign/core/Selector";
import { useRouter } from "next/navigation";
import { type ReactNode, useTransition } from "react";

import { languageLabel } from "@/lib/languages";
import { STATES, type WorklistState, isState, worklistHref } from "@/lib/translation-worklist";
import type { Kind } from "@/lib/translations";
import { useFormat } from "@/lib/use-format";
import { useT } from "@/lib/use-t";

export function WorklistControls({
  languages,
  language,
  state,
  kind,
  children,
}: {
  /** The store's other languages; `published` is unknown without `stores:read`. */
  languages: { language: string; published?: boolean }[];
  language: string;
  state: WorklistState;
  /** The table's kind filter, kept across a change of language or state. */
  kind: Kind | undefined;
  children: ReactNode;
}) {
  const t = useT<"translations">();
  const format = useFormat();
  const router = useRouter();
  const [isPending, startTransition] = useTransition();

  // Built from what the server read rather than the URL, so the page starts again at its
  // first page.
  const go = (changes: { language?: string; state?: WorklistState }) => {
    const url = worklistHref({ language, state, kind, ...changes });
    startTransition(() => router.replace(url, { scroll: false }));
  };

  const options = languages.map((l) => ({
    value: l.language,
    label:
      l.published === false
        ? t("translations.worklist_language_unpublished", {
            language: languageLabel(format, l.language),
          })
        : languageLabel(format, l.language),
  }));

  return (
    <>
      <HStack gap={3} vAlign="end" wrap="wrap">
        <Selector
          label={t("translations.worklist_language")}
          options={options}
          value={language}
          onChange={(value: string) => go({ language: value })}
          hasSearch={options.length > 8}
          width={280}
        />
        <SegmentedControl
          label={t("translations.worklist_state")}
          value={state}
          onChange={(value) => {
            if (isState(value)) go({ state: value });
          }}
        >
          {STATES.map((s) => (
            <SegmentedControlItem key={s} value={s} label={t(`translations.worklist_state_${s}`)} />
          ))}
        </SegmentedControl>
      </HStack>
      <div className="pending-content" aria-busy={isPending} data-pending={isPending || undefined}>
        {children}
      </div>
    </>
  );
}
