"use client";

// The discount form: its sections beside the sticky summary. It holds no draft of its
// own: the page keeps the draft and its errors, saves (a JSON Merge Patch of
// `changedSections`) and previews the write for the server's summary. The variant,
// product and customer pickers search the API themselves and hand back names.

import { Banner } from "@astryxdesign/core/Banner";
import { VStack } from "@astryxdesign/core/Layout";

import { useCurrencies } from "@/components/currencies";
import { useTimeZone } from "@/components/date-time";
import type { DiscountDraft } from "@/lib/discounts/draft";
import { draftSummary } from "@/lib/discounts/summary";
import type { FieldErrors } from "@/lib/discounts/validate";
import { toLocalInput } from "@/lib/price-lists";

import { AppliesToSection } from "./applies-to-section";
import { CombinationsSection, type LiveDiscount } from "./combinations-section";
import { ConditionsSection } from "./conditions-section";
import type { DiscountOptions } from "./fields";
import { LimitsSection } from "./limits-section";
import { MethodSection } from "./method-section";
import { ScheduleSection } from "./schedule-section";
import { SummaryPanel } from "./summary-panel";
import { ValueSection } from "./value-section";

export function DiscountForm({
  draft,
  errors,
  onChange,
  options,
  names,
  onNames,
  isNew,
  isReadOnly = false,
  live,
  selfId,
  serverSummary,
  isRefreshing,
  usage,
  now,
}: {
  draft: DiscountDraft;
  /** By draft path (`fieldErrors`); "" holds what no field shows. */
  errors: FieldErrors;
  /** The next draft and the paths that changed, whose errors the page clears. */
  onChange: (draft: DiscountDraft, paths: string[]) => void;
  options: DiscountOptions;
  /** Names of the products, variants and customers the draft names, by ID. */
  names: Readonly<Record<string, string>>;
  onNames: (names: Record<string, string>) => void;
  isNew: boolean;
  isReadOnly?: boolean;
  /** The store's live discounts, for what each combination box affects. */
  live?: readonly LiveDiscount[];
  selfId?: string;
  serverSummary?: string | null;
  isRefreshing?: boolean;
  usage?: string;
  /** The instant (ms) the summary's phase is judged at. */
  now: number;
}) {
  const currencies = useCurrencies();
  const timeZone = useTimeZone();
  const allNames = {
    ...Object.fromEntries(options.markets.map((m) => [m.id, m.name])),
    ...Object.fromEntries(
      [...options.channels, ...options.groups, ...options.categories].map((o) => [
        o.value,
        o.label,
      ]),
    ),
    ...names,
  };
  const preview = draftSummary(draft, {
    markets: options.markets,
    currencies,
    names: allNames,
    today: toLocalInput(new Date(now).toISOString(), timeZone).slice(0, 10),
  });
  const section = { draft, errors, onChange, isReadOnly };

  return (
    <div className="discount-layout">
      <VStack gap={5}>
        {errors[""] && <Banner status="error" title={errors[""]} />}
        <MethodSection {...section} isNew={isNew} />
        <ValueSection {...section} markets={options.markets} />
        <AppliesToSection
          {...section}
          categories={options.categories}
          names={allNames}
          onNames={onNames}
        />
        <ConditionsSection {...section} options={options} names={allNames} onNames={onNames} />
        <CombinationsSection
          draft={draft}
          onChange={onChange}
          isReadOnly={isReadOnly}
          live={live}
          selfId={selfId}
        />
        <LimitsSection {...section} />
        <ScheduleSection {...section} />
      </VStack>
      <SummaryPanel
        draft={draft}
        preview={preview}
        serverSummary={serverSummary}
        isRefreshing={isRefreshing}
        timeZone={timeZone}
        now={now}
        usage={usage}
      />
    </div>
  );
}
