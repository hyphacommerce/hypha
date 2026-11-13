"use client";

// The form's sticky summary (`.discount-summary`): the discount in one line, its method, classes and phase.
// The server's summary (the words buyers see) replaces the instant preview once the
// store has answered with it (`POST /admin/discounts/summary`).

import { Card } from "@astryxdesign/core/Card";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Text } from "@astryxdesign/core/Text";

import { type DiscountDraft, classesOf, phaseOf } from "@/lib/discounts/draft";
import { fromLocalInput } from "@/lib/price-lists";
import { useFormat } from "@/lib/use-format";
import { useT } from "@/lib/use-t";

import { DiscountStatusBadge } from "./status-badge";

const CLASS_LABELS = {
  product: "common.products",
  order: "common.order",
  shipping: "common.shipping",
} as const;

/** How the store's own summary (the API's English) marks a draft. */
const API_DRAFT = "Draft";

export function SummaryPanel({
  draft,
  preview,
  serverSummary,
  isRefreshing = false,
  timeZone,
  now,
  usage,
}: {
  draft: DiscountDraft;
  /** `draftSummary` of the draft: shown until the server's arrives. */
  preview: string;
  /** The store's summary of this draft, once it answered with one. */
  serverSummary?: string | null;
  /** The store's summary is on its way. */
  isRefreshing?: boolean;
  timeZone: string;
  /** The instant (ms) the phase is judged at. */
  now: number;
  /** "12 of 100 uses": a saved discount's count, from the API. */
  usage?: string;
}) {
  const t = useT<"discounts">();
  const format = useFormat();
  const starts = draft.schedule.startsAt ? fromLocalInput(draft.schedule.startsAt, timeZone) : null;
  const ends = draft.schedule.endsAt ? fromLocalInput(draft.schedule.endsAt, timeZone) : null;
  const phase = phaseOf(draft.status, starts, ends, now);
  const classes = classesOf(draft.kind);

  return (
    <div className="discount-summary">
      <Card>
        <VStack gap={3}>
          <HStack gap={2} hAlign="between" vAlign="center">
            <Text type="display-3" as="h2">
              {t("common.summary")}
            </Text>
            <DiscountStatusBadge phase={phase} />
          </HStack>
          <Text weight="semibold">{draft.title.trim() || t("discounts.panel_untitled")}</Text>
          <Text color="secondary">
            {draft.method === "codes"
              ? draft.code.trim()
                ? t("discounts.panel_code", { code: draft.code.trim().toUpperCase() })
                : t("discounts.method_codes")
              : t("discounts.method_automatic")}
            {" · "}
            {classes.length > 0
              ? format.list(
                  classes.map((c) => t(CLASS_LABELS[c])),
                  "unit",
                )
              : t("discounts.panel_nothing_off")}
          </Text>
          <div aria-live="polite" style={{ opacity: isRefreshing ? 0.6 : 1 }}>
            <VStack gap={1}>
              {(serverSummary ?? preview)
                .split(" · ")
                .filter((part) => part !== API_DRAFT && part !== t("common.draft"))
                .map((part) => (
                  <Text key={part}>{part}</Text>
                ))}
            </VStack>
          </div>
          {!serverSummary && (
            <Text type="supporting" color="secondary">
              {t("discounts.panel_preview_note")}
            </Text>
          )}
          {usage && (
            <Text type="supporting" color="secondary">
              {usage}
            </Text>
          )}
        </VStack>
      </Card>
    </div>
  );
}
