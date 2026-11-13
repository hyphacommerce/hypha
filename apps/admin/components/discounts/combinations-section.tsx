"use client";

// Which other discounts this one may apply together with, by class. Two discounts
// combine only when each allows every class of the other; under each box, the live
// discounts that choice lets it combine with.

import { Card } from "@astryxdesign/core/Card";
import { CheckboxInput } from "@astryxdesign/core/CheckboxInput";
import { VStack } from "@astryxdesign/core/Layout";
import { Text } from "@astryxdesign/core/Text";

import {
  CLASSES,
  type ClassSettings,
  type DiscountClass,
  classesOf,
  combinedClasses,
  compatible,
} from "@/lib/discounts/draft";

import { useFormat } from "@/lib/use-format";
import { useT } from "@/lib/use-t";

import type { SectionProps } from "./fields";

/** A live discount, as the form weighs combining with it. */
export type LiveDiscount = ClassSettings & { id: string; title: string };

const NONE: readonly LiveDiscount[] = [];

export function CombinationsSection({
  draft,
  onChange,
  isReadOnly,
  live = NONE,
  selfId,
}: Omit<SectionProps, "errors"> & {
  /** The store's live discounts; none shown when not given. */
  live?: readonly LiveDiscount[];
  /** This discount's ID, to leave it out of `live`. */
  selfId?: string;
}) {
  const t = useT<"discounts">();
  const format = useFormat();
  /** "Lets it combine with A, B, C and 2 more.": the first three by title. */
  const letsCombine = (discounts: readonly LiveDiscount[]) => {
    const shown = discounts.slice(0, 3).map((d) => d.title);
    return t("discounts.combinations_lets_combine", {
      titles: format.list(shown, "unit"),
      more: discounts.length - shown.length,
    });
  };
  const own: ClassSettings = {
    classes: classesOf(draft.kind),
    combinesWith: combinedClasses(draft),
  };
  const others = live.filter((d) => d.id !== selfId);

  /** The live discounts of `c` this one combines with when `c` is allowed. */
  const affected = (c: DiscountClass) => {
    const allowed = { ...own, combinesWith: [...new Set([...own.combinesWith, c])] };
    return others.filter((d) => d.classes.includes(c) && compatible(allowed, d));
  };

  return (
    <Card>
      <VStack gap={4}>
        <VStack gap={1}>
          <Text type="display-3" as="h2">
            {t("discounts.combinations_title")}
          </Text>
          <Text type="supporting" color="secondary">
            {t("discounts.combinations_hint")}
          </Text>
        </VStack>
        {CLASSES.map((c) => {
          const combined = affected(c);
          const ofClass = others.filter((d) => d.classes.includes(c));
          const note =
            combined.length > 0
              ? letsCombine(combined)
              : ofClass.length > 0
                ? t("discounts.combinations_changes_nothing")
                : undefined;
          return (
            <CheckboxInput
              key={c}
              label={t(`discounts.combinations_${c}`)}
              description={note}
              value={draft.combinesWith[c]}
              onChange={(on) =>
                onChange({ ...draft, combinesWith: { ...draft.combinesWith, [c]: on } }, [
                  "combinesWith",
                ])
              }
              isDisabled={isReadOnly}
            />
          );
        })}
      </VStack>
    </Card>
  );
}
