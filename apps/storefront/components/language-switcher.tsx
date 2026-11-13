"use client";

// The language switcher, beside the currencies: the languages the shopper's country is
// offered in, each named in itself. Picking one goes to the same page, its listing's
// filters and all, under the language's URL, and remembers it (the `language` cookie)
// for the next bare URL. Hidden where the country has one language.

import { SegmentedControl, SegmentedControlItem } from "@astryxdesign/core/SegmentedControl";
import { Selector } from "@astryxdesign/core/Selector";
import { useTransition } from "react";

import { setLanguage } from "@/app/[country]/actions";

import { useT } from "@/lib/use-t";

import { pagePath, useLocale } from "./locale";
import { useNavigationProgress } from "./navigation-progress";

export type LanguageOption = {
  /** Where the page is in this language: its locale segment (`en-de`). */
  segment: string;
  /** Its name in itself ("Deutsch"). */
  name: string;
};

export function LanguageSwitcher({
  languages,
  layout = "menu",
}: {
  languages: LanguageOption[];
  /** As the currency switcher's: a dropdown, or every language at once. */
  layout?: "menu" | "segments";
}) {
  const t = useT();
  const current = useLocale();
  const [pending, start] = useTransition();
  const progress = useNavigationProgress();
  if (languages.length < 2) return null;
  const pick = (segment: string | null) => {
    if (!segment || segment === current) return;
    progress.start();
    start(() => setLanguage({ locale: current, target: segment, path: pagePath() }));
  };
  if (layout === "segments") {
    return (
      <SegmentedControl
        label={t("header.language")}
        size="sm"
        value={current}
        onChange={pick}
        isDisabled={pending}
      >
        {languages.map((l) => (
          <SegmentedControlItem key={l.segment} value={l.segment} label={l.name} />
        ))}
      </SegmentedControl>
    );
  }
  return (
    <Selector
      label={t("header.language")}
      isLabelHidden
      variant="ghost"
      size="sm"
      isLoading={pending}
      value={current}
      options={languages.map((l) => ({ value: l.segment, label: l.name }))}
      onChange={pick}
    />
  );
}
