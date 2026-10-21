"use client";

// A listing's controls (design note 0024, §7): facets as checkbox lists and a price
// range beside the results (in a bottom sheet on phones), and the sort. Every change
// goes to a new URL, so the state can be linked to and the back button works. The URL
// changes in place (`history.pushState`, which Next keeps `useSearchParams` in step
// with): no page is re-rendered on the server, only the results are read again
// (`Listing`), so the controls stay put and nothing flashes.

import { Button } from "@astryxdesign/core/Button";
import { CheckboxList, CheckboxListItem } from "@astryxdesign/core/CheckboxList";
import { Divider } from "@astryxdesign/core/Divider";
import { Icon } from "@astryxdesign/core/Icon";
import { Selector } from "@astryxdesign/core/Selector";
import { HStack, StackItem, VStack } from "@astryxdesign/core/Stack";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import { AdjustmentsHorizontalIcon } from "@heroicons/react/24/outline";
import { Suspense, lazy, useState } from "react";

import {
  SORTS,
  type ListingState,
  type Sort,
  changed,
  facetName,
  isSort,
  listingHref,
} from "@/lib/listing";

/** The phones' filter sheet, loaded the first time it opens: wide screens never need
 * it, and it's the listing's heaviest piece. */
const FilterSheet = lazy(() => import("./listing-sheet"));

export type Facet = {
  key: string;
  label?: string | null;
  /** `label`: what to show for the value (a choice field's label), when it isn't the
   * value itself. */
  values: { value: string; label?: string | null; count: number }[];
};

export type Props = {
  base: string;
  state: ListingState;
  facets: Facet[];
  /** The results' price range in major units, as hints for the range fields. */
  priceHint?: { min: string; max: string };
  currency?: string;
};

/** A facet's heading: the merchant's label for an option or a custom field (design note
 * 0033, §9), else its name. */
function facetLabel(facet: Facet): string {
  const labelled = facet.key.startsWith("option.") || facet.key.startsWith("field.");
  return labelled && facet.label ? facet.label : facetName(facet.key);
}

/** How many values a facet shows before "Show all". */
const SHOWN = 8;

/**
 * Moves the listing to `state` in place: the URL changes without a navigation, and the
 * listing reads the new page. Next's router patches `pushState`, so `useSearchParams`
 * (and the back button) follow.
 */
export function go(base: string, state: ListingState) {
  window.history.pushState(null, "", listingHref(base, state));
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
        label={facetLabel(facet)}
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
            label={v.label ?? v.value}
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
  const apply = (change: Partial<ListingState>) => {
    go(base, changed(state, change));
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

/** Above the results: the count, the sort, and (on phones, by CSS) the filters in a
 * sheet. */
export function ListingToolbar(props: Props & { total: number; sort: Sort; isBusy: boolean }) {
  const [sheet, setSheet] = useState(false);
  // Once opened, the sheet stays mounted (closed), so it opens again at once.
  const [sheetLoaded, setSheetLoaded] = useState(false);
  const active = props.state.filters.length + (props.state.min || props.state.max ? 1 : 0);
  return (
    <HStack gap={3} vAlign="center" hAlign="between" wrap="wrap">
      <Text color="secondary">
        {props.total} {props.total === 1 ? "product" : "products"}
      </Text>
      <HStack gap={2} vAlign="center">
        <div className="shop-listing-filters">
          <Button
            label={active > 0 ? `Filters (${active})` : "Filters"}
            icon={<Icon icon={AdjustmentsHorizontalIcon} />}
            clickAction={() => {
              setSheetLoaded(true);
              setSheet(true);
            }}
          />
          {sheetLoaded && (
            <Suspense fallback={null}>
              <FilterSheet {...props} isOpen={sheet} onOpenChange={setSheet} />
            </Suspense>
          )}
        </div>
        <Selector
          label="Sort"
          isLabelHidden
          variant="secondary"
          isLoading={props.isBusy}
          value={props.sort}
          options={SORTS.filter((s) => s.value !== "relevance" || props.state.q).map((s) => ({
            value: s.value,
            label: s.label,
          }))}
          onChange={(sort) => {
            if (isSort(sort)) go(props.base, changed(props.state, { sort }));
          }}
        />
      </HStack>
    </HStack>
  );
}
