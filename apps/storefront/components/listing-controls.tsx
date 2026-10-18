"use client";

// A listing's controls (design note 0024, §7): facets as checkbox lists and a price
// range beside the results (in a bottom sheet on phones), and the sort. Every change
// goes to a new URL, so the server renders the results and the state can be linked to.

import { BottomSheet } from "@astryxdesign/core/BottomSheet";
import { Button } from "@astryxdesign/core/Button";
import { CheckboxList, CheckboxListItem } from "@astryxdesign/core/CheckboxList";
import { Divider } from "@astryxdesign/core/Divider";
import { Icon } from "@astryxdesign/core/Icon";
import { Selector } from "@astryxdesign/core/Selector";
import { HStack, StackItem, VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import { AdjustmentsHorizontalIcon } from "@heroicons/react/24/outline";
import { useMediaQuery } from "@astryxdesign/core/hooks";
import { useRouter } from "next/navigation";
import { useState, useTransition } from "react";

import {
  SORTS,
  type ListingState,
  type Sort,
  changed,
  facetName,
  isSort,
  listingHref,
} from "@/lib/listing";

import { CloseOnNavigate } from "./close-on-navigate";

export type Facet = {
  key: string;
  label?: string | null;
  values: { value: string; count: number }[];
};

type Props = {
  base: string;
  state: ListingState;
  facets: Facet[];
  /** The results' price range in major units, as hints for the range fields. */
  priceHint?: { min: string; max: string };
  currency?: string;
};

/** How many values a facet shows before "Show all". */
const SHOWN = 8;

function useGo(base: string) {
  const router = useRouter();
  const [pending, start] = useTransition();
  const go = (state: ListingState) => start(() => router.push(listingHref(base, state)));
  return { go, pending };
}

function FacetList({
  facet,
  selected,
  onChange,
}: {
  facet: Facet;
  selected: string[];
  onChange: (values: string[]) => void;
}) {
  const [open, setOpen] = useState(false);
  const values = open ? facet.values : facet.values.slice(0, SHOWN);
  const prefix = `${facet.key}:`;
  return (
    <VStack gap={1}>
      <CheckboxList
        label={facet.key.startsWith("option.") && facet.label ? facet.label : facetName(facet.key)}
        density="compact"
        value={selected.filter((f) => f.startsWith(prefix))}
        onChange={(picked) =>
          onChange([...selected.filter((f) => !f.startsWith(prefix)), ...picked])
        }
      >
        {values.map((v) => (
          <CheckboxListItem
            key={v.value}
            value={`${prefix}${v.value}`}
            label={v.value}
            endContent={<Text type="supporting">{v.count}</Text>}
          />
        ))}
      </CheckboxList>
      {facet.values.length > SHOWN && (
        <div>
          <Button
            label={open ? "Show fewer" : `Show all ${facet.values.length}`}
            variant="ghost"
            size="sm"
            clickAction={() => setOpen(!open)}
          />
        </div>
      )}
    </VStack>
  );
}

/** Whether a price field holds an amount, or nothing. */
function isPrice(v: string): boolean {
  return v.trim() === "" || /^\d+(\.\d+)?$/.test(v.trim());
}

function PriceRange({
  state,
  priceHint,
  currency,
  onApply,
}: {
  state: ListingState;
  priceHint?: { min: string; max: string };
  currency?: string;
  onApply: (min?: string, max?: string) => void;
}) {
  const [min, setMin] = useState(state.min ?? "");
  const [max, setMax] = useState(state.max ?? "");
  const ok = isPrice(min) && isPrice(max);
  return (
    <VStack gap={2}>
      <Text type="label">Price{currency ? ` (${currency})` : ""}</Text>
      <HStack gap={2} vAlign="center">
        <StackItem size="fill">
          <TextInput
            label="Lowest price"
            isLabelHidden
            placeholder={priceHint ? priceHint.min : "Min"}
            value={min}
            onChange={setMin}
          />
        </StackItem>
        <Text color="secondary">–</Text>
        <StackItem size="fill">
          <TextInput
            label="Highest price"
            isLabelHidden
            placeholder={priceHint ? priceHint.max : "Max"}
            value={max}
            onChange={setMax}
          />
        </StackItem>
      </HStack>
      <div>
        <Button
          label="Apply price"
          size="sm"
          isDisabled={!ok}
          clickAction={() => onApply(min.trim() || undefined, max.trim() || undefined)}
        />
      </div>
    </VStack>
  );
}

/** The facets and the price range. */
export function FacetPanel({
  base,
  state,
  facets,
  priceHint,
  currency,
  onDone,
}: Props & {
  onDone?: () => void;
}) {
  const { go } = useGo(base);
  const apply = (change: Partial<ListingState>) => {
    go(changed(state, change));
    onDone?.();
  };
  const narrowed = state.filters.length > 0 || state.min || state.max;
  return (
    <VStack gap={5}>
      {facets.map((facet, i) => (
        <VStack key={facet.key} gap={5}>
          {i > 0 && <Divider />}
          <FacetList
            facet={facet}
            selected={state.filters}
            onChange={(filters) => apply({ filters })}
          />
        </VStack>
      ))}
      {facets.length > 0 && <Divider />}
      <PriceRange
        state={state}
        priceHint={priceHint}
        currency={currency}
        onApply={(min, max) => apply({ min, max })}
      />
      {narrowed && (
        <div>
          <Button
            label="Clear filters"
            variant="ghost"
            clickAction={() => apply({ filters: [], min: undefined, max: undefined })}
          />
        </div>
      )}
    </VStack>
  );
}

/** Above the results: the count, the sort, and (on phones) the filters in a sheet. */
export function ListingToolbar(props: Props & { total: number; sort: Sort }) {
  const isWide = useMediaQuery("(min-width: 900px)", true);
  const { go, pending } = useGo(props.base);
  const [sheet, setSheet] = useState(false);
  const active = props.state.filters.length + (props.state.min || props.state.max ? 1 : 0);
  return (
    <HStack gap={3} vAlign="center" hAlign="between" wrap="wrap">
      <Text color="secondary">
        {props.total} {props.total === 1 ? "product" : "products"}
      </Text>
      <HStack gap={2} vAlign="center">
        {!isWide && (
          <>
            <Button
              label={active > 0 ? `Filters (${active})` : "Filters"}
              icon={<Icon icon={AdjustmentsHorizontalIcon} />}
              clickAction={() => setSheet(true)}
            />
            {/* Leaving the listing (a product, another category) closes it; filtering here,
              which changes only the query, doesn't. */}
            <CloseOnNavigate pathOnly onNavigate={() => setSheet(false)} />
            <BottomSheet label="Filters" isOpen={sheet} onOpenChange={setSheet} height="tall">
              <FacetPanel {...props} onDone={() => setSheet(false)} />
            </BottomSheet>
          </>
        )}
        <Selector
          label="Sort"
          isLabelHidden
          variant="secondary"
          isLoading={pending}
          value={props.sort}
          options={SORTS.filter((s) => s.value !== "relevance" || props.state.q).map((s) => ({
            value: s.value,
            label: s.label,
          }))}
          onChange={(sort) => {
            if (isSort(sort)) go(changed(props.state, { sort }));
          }}
        />
      </HStack>
    </HStack>
  );
}
