"use client";

// A discount's form with its draft, errors and the store's own summary: a new one from a
// template, created in one go, or a saved one, whose changed sections are saved as a
// JSON Merge Patch from the save bar. The summary is the API's: each change is sent to
// `POST /admin/discounts/summary` once typing stops, the instant one shown meanwhile and
// whenever a rule is broken (the rules it names are marked on their fields).

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Text } from "@astryxdesign/core/Text";
import { useEffect, useRef, useState, useTransition } from "react";

import { useCurrencies } from "@/components/currencies";
import { useTimeZone } from "@/components/date-time";
import { DiscountForm } from "@/components/discounts/discount-form";
import type { LiveDiscount } from "@/components/discounts/combinations-section";
import type { DiscountOptions } from "@/components/discounts/fields";
import { TemplatesPicker } from "@/components/discounts/templates-picker";
import { SaveBar } from "@/components/save-bar";
import { type DiscountDraft, isDirty } from "@/lib/discounts/draft";
import { type TemplateKey, templateOf } from "@/lib/discounts/templates";
import {
  type CheckContext,
  type FieldErrors,
  clearErrors,
  fieldErrors,
  problemsFromDetails,
} from "@/lib/discounts/validate";
import {
  type Discount,
  apiPathMapper,
  createBody,
  draftOf,
  patchBody,
  refusalProblems,
} from "@/lib/discounts/wire";

import { type SummaryAnswer, createDiscount, discountSummary, updateDiscount } from "./actions";

/** How long typing must stop before the store is asked for its summary. */
const SUMMARY_AFTER_MS = 600;

type Common = {
  options: DiscountOptions;
  /** Names of the products, variants and customers the discount names, by ID. */
  names: Record<string, string>;
  live: readonly LiveDiscount[];
  /** The instant (ms) the page was rendered: the summary's phase is judged then. */
  now: number;
};

/**
 * The store's summary of the discount `write` would make, once typing stops: the text
 * (`fallback` while there's no request to send; null while a rule is broken), the
 * problems the store found in `draft` by field, and whether an answer is on its way.
 */
function useServerSummary(
  write: Parameters<typeof discountSummary>[0] | null,
  fallback: string | null,
  draft: DiscountDraft | null,
) {
  const [answer, setAnswer] = useState<{ for: string; answer: SummaryAnswer | null }>();
  const latest = useRef(0);
  const key = write ? JSON.stringify(write) : null;

  useEffect(() => {
    if (!write || key === null) return;
    const request = ++latest.current;
    const timer = setTimeout(async () => {
      const next = await discountSummary(write).catch(() => null);
      if (request === latest.current) setAnswer({ for: key, answer: next });
    }, SUMMARY_AFTER_MS);
    return () => clearTimeout(timer);
    // `key` stands for `write`: a new object with the same contents isn't a change.
    // oxlint-disable-next-line react-hooks/exhaustive-deps -- write is compared by its JSON (key)
  }, [key]);

  const none: FieldErrors = {};
  if (key === null) return { text: fallback, errors: none, isRefreshing: false };
  if (answer?.for === key) {
    const problems = answer.answer?.problems ?? [];
    return {
      text: answer.answer?.summary ?? null,
      errors:
        draft && problems.length > 0
          ? fieldErrors(problemsFromDetails(problems, apiPathMapper(draft)))
          : none,
      isRefreshing: false,
    };
  }
  return { text: answer?.answer?.summary ?? fallback, errors: none, isRefreshing: true };
}

function useCheckContext(options: DiscountOptions): CheckContext {
  return { markets: options.markets, currencies: useCurrencies(), timeZone: useTimeZone() };
}

/** A new discount: a template first, then the form, created with its first code. */
export function NewDiscount({
  options,
  names: initialNames,
  live,
  now,
  initial,
}: Common & {
  /** A template chosen by the URL (`?template=`). */
  initial: TemplateKey | null;
}) {
  const ctx = useCheckContext(options);
  const [template, setTemplate] = useState<TemplateKey | null>(initial);
  const [draft, setDraft] = useState<DiscountDraft | null>(
    () => (initial && templateOf(initial)?.make()) || null,
  );
  const [errors, setErrors] = useState<FieldErrors>({});
  const [names, setNames] = useState(initialNames);
  const [isPending, startTransition] = useTransition();

  const built = draft ? createBody(draft, ctx) : null;
  const preview = useServerSummary(
    built && "body" in built ? { id: null, body: built.body } : null,
    null,
    draft,
  );

  if (!draft) {
    return (
      <VStack gap={3}>
        <Text type="display-3" as="h2">
          Start from
        </Text>
        <TemplatesPicker
          value={template}
          onChoose={(key, next) => {
            setTemplate(key);
            setDraft(next);
          }}
        />
      </VStack>
    );
  }

  const create = () =>
    startTransition(async () => {
      if (!built) return;
      if ("problems" in built) {
        setErrors(fieldErrors(built.problems));
        return;
      }
      const result = await createDiscount(built.body);
      if (result.refusal) setErrors(fieldErrors(refusalProblems(result.refusal, draft)));
    });

  return (
    <VStack gap={5}>
      <HStack gap={2} vAlign="center" wrap="wrap">
        <Text color="secondary">From the template {templateOf(template ?? "")?.label ?? ""}.</Text>
        <Button
          label="Choose another"
          variant="ghost"
          size="sm"
          onClick={() => {
            setDraft(null);
            setErrors({});
          }}
        />
      </HStack>
      <DiscountForm
        draft={draft}
        errors={{ ...preview.errors, ...errors }}
        onChange={(next, paths) => {
          setDraft(next);
          setErrors((before) => clearErrors(before, ["", ...paths]));
        }}
        options={options}
        names={names}
        onNames={(more) => setNames((before) => ({ ...before, ...more }))}
        isNew
        live={live}
        serverSummary={preview.text}
        isRefreshing={preview.isRefreshing}
        now={now}
      />
      <HStack gap={2} hAlign="end">
        <Button label="Cancel" variant="ghost" href="/discounts" />
        <Button label="Create discount" variant="primary" isLoading={isPending} onClick={create} />
      </HStack>
    </VStack>
  );
}

/** A saved discount's settings. */
export function EditDiscount({
  discount,
  options,
  names: initialNames,
  live,
  now,
  isReadOnly,
}: Common & { discount: Discount; isReadOnly: boolean }) {
  const ctx = useCheckContext(options);
  const [saved] = useState(() => draftOf(discount, ctx));
  const [draft, setDraft] = useState(saved);
  const [errors, setErrors] = useState<FieldErrors>({});
  const [error, setError] = useState<string>();
  const [names, setNames] = useState(initialNames);
  const [isPending, startTransition] = useTransition();
  const dirty = isDirty(saved, draft);

  const built = patchBody(saved, draft, ctx);
  const changes = "body" in built && Object.keys(built.body).length > 0 ? built.body : null;
  const preview = useServerSummary(
    !isReadOnly && changes ? { id: discount.id, body: changes } : null,
    discount.summary,
    draft,
  );
  // A draft with a broken rule has no store summary: the saved one would be wrong for it.
  const serverSummary = "problems" in built ? null : preview.text;

  const save = () =>
    startTransition(async () => {
      if ("problems" in built) {
        setErrors(fieldErrors(built.problems));
        setError("Fix the fields marked below.");
        return;
      }
      const result = await updateDiscount(discount.id, built.body);
      if (result.refusal) {
        const problems = refusalProblems(result.refusal, draft);
        setErrors(fieldErrors(problems));
        setError(problems.some((p) => p.path !== "") ? "Fix the fields marked below." : undefined);
      } else {
        setError(undefined);
      }
    });

  const uses =
    discount.limits.total == null
      ? `Used on ${discount.used.toLocaleString("en")} ${discount.used === 1 ? "order" : "orders"}`
      : `${discount.used.toLocaleString("en")} of ${discount.limits.total.toLocaleString("en")} uses`;

  return (
    <VStack gap={4}>
      <SaveBar
        isDirty={dirty}
        isPending={isPending}
        error={error}
        onSave={save}
        onDiscard={() => {
          setDraft(saved);
          setErrors({});
          setError(undefined);
        }}
      />
      {isReadOnly && (
        <Banner status="info" title="Your role can see discounts but not change them." />
      )}
      <DiscountForm
        draft={draft}
        errors={{ ...preview.errors, ...errors }}
        onChange={(next, paths) => {
          setDraft(next);
          setErrors((before) => clearErrors(before, ["", ...paths]));
        }}
        options={options}
        names={names}
        onNames={(more) => setNames((before) => ({ ...before, ...more }))}
        isNew={false}
        isReadOnly={isReadOnly}
        live={live}
        selfId={discount.id}
        serverSummary={serverSummary}
        isRefreshing={preview.isRefreshing}
        usage={uses}
        now={now}
      />
    </VStack>
  );
}
