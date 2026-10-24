"use client";

// Picking variants or customers for a discount: a dialog that lists them as soon as it
// opens, narrows them as you type, marks the ones already chosen and hands back the
// picked ones with their names (for the tokens and the summary).

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { CheckboxInput } from "@astryxdesign/core/CheckboxInput";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Skeleton } from "@astryxdesign/core/Skeleton";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import { MagnifyingGlassIcon } from "@heroicons/react/24/outline";
import { useEffect, useRef, useState } from "react";

import { type PickerRow, type PickerRows, searchCustomers, searchVariants } from "./picker-actions";

export type { PickerRow };

const SOURCES = {
  variants: {
    load: searchVariants,
    placeholder: "Search by product title, handle or SKU",
    noun: "variants",
  },
  customers: {
    load: searchCustomers,
    placeholder: "Search by name or email",
    noun: "customers",
  },
} satisfies Record<
  string,
  { load: (q: string, cursor?: string) => Promise<PickerRows>; placeholder: string; noun: string }
>;

export function SearchPicker({
  source,
  title,
  chosen,
  onConfirm,
  onClose,
}: {
  source: keyof typeof SOURCES;
  title: string;
  /** IDs already chosen: shown ticked and not pickable. */
  chosen: readonly string[];
  onConfirm: (rows: PickerRow[]) => void;
  onClose: () => void;
}) {
  const { load, placeholder, noun } = SOURCES[source];
  const [q, setQ] = useState("");
  const [rows, setRows] = useState<PickerRow[]>();
  const [next, setNext] = useState<string | null>(null);
  const [error, setError] = useState<string>();
  const [shownFor, setShownFor] = useState<string>();
  const [isLoadingMore, setIsLoadingMore] = useState(false);
  const [picked, setPicked] = useState<Map<string, PickerRow>>(() => new Map());
  const isLoading = shownFor !== q;
  // Only the latest search's answer counts: typing fast starts several.
  const latest = useRef(0);

  useEffect(() => {
    const request = ++latest.current;
    const timer = setTimeout(
      async () => {
        const page = await load(q);
        if (request !== latest.current) return;
        setRows(page.items);
        setNext(page.nextCursor);
        setError(page.error);
        setShownFor(q);
      },
      q ? 250 : 0,
    );
    return () => clearTimeout(timer);
  }, [q, load]);

  const loadMore = async () => {
    if (!next) return;
    const request = latest.current;
    setIsLoadingMore(true);
    let page;
    try {
      page = await load(q, next);
    } finally {
      setIsLoadingMore(false);
    }
    if (request !== latest.current) return;
    setRows((before) => [...(before ?? []), ...page.items]);
    setNext(page.nextCursor);
    setError(page.error);
  };

  const toggle = (row: PickerRow) =>
    setPicked((before) => {
      const after = new Map(before);
      if (before.has(row.id)) after.delete(row.id);
      else after.set(row.id, row);
      return after;
    });

  return (
    <Dialog
      isOpen
      onOpenChange={(isOpen) => !isOpen && onClose()}
      width={600}
      maxHeight="85dvh"
      purpose="form"
      padding={0}
    >
      <div style={{ padding: "16px 16px 0" }}>
        <DialogHeader title={title} onOpenChange={(isOpen) => !isOpen && onClose()} />
      </div>
      <VStack gap={3} padding={4}>
        <TextInput
          label={`Search ${noun}`}
          isLabelHidden
          placeholder={placeholder}
          startIcon={MagnifyingGlassIcon}
          value={q}
          onChange={setQ}
          hasClear
          hasAutoFocus
        />
        {error && <Banner status="error" title={error} />}
      </VStack>

      <div className="picker-list" aria-busy={isLoading}>
        {isLoading && !rows ? (
          <VStack gap={2} padding={4}>
            {Array.from({ length: 6 }, (_, i) => (
              <Skeleton key={i} index={i} height={44} />
            ))}
          </VStack>
        ) : rows && rows.length === 0 ? (
          <VStack gap={1} padding={6} hAlign="center">
            <Text weight="medium">{q ? `No ${noun} match` : `No ${noun} yet`}</Text>
          </VStack>
        ) : (
          <div style={{ opacity: isLoading ? 0.6 : 1 }}>
            {rows?.map((row) => {
              const already = chosen.includes(row.id);
              const isPicked = picked.has(row.id);
              return (
                // The whole row is the checkbox's hit area for the mouse; keyboards and
                // screen readers use the checkbox itself.
                <div
                  key={row.id}
                  role="presentation"
                  className="picker-row picker-row-plain"
                  data-disabled={already || undefined}
                  data-picked={isPicked || undefined}
                  onClick={() => !already && toggle(row)}
                >
                  <div role="presentation" onClick={(e) => e.stopPropagation()}>
                    <CheckboxInput
                      label={`Pick ${row.label}`}
                      isLabelHidden
                      value={already || isPicked}
                      isDisabled={already}
                      disabledMessage={already ? "Already chosen" : undefined}
                      onChange={() => toggle(row)}
                    />
                  </div>
                  <VStack gap={0}>
                    <Text weight="medium" maxLines={1}>
                      {row.label}
                    </Text>
                    {row.detail && (
                      <Text type="supporting" color="secondary" maxLines={1}>
                        {row.detail}
                      </Text>
                    )}
                  </VStack>
                </div>
              );
            })}
            {next && (
              <HStack hAlign="center" padding={3}>
                <Button
                  label="Load more"
                  variant="ghost"
                  size="sm"
                  isLoading={isLoadingMore}
                  onClick={loadMore}
                />
              </HStack>
            )}
          </div>
        )}
      </div>

      <HStack gap={2} hAlign="between" vAlign="center" padding={4}>
        <Text type="supporting" color="secondary">
          {picked.size === 0 ? `Pick one or more ${noun}` : `${picked.size} selected`}
        </Text>
        <HStack gap={2}>
          <Button label="Cancel" variant="ghost" onClick={onClose} />
          <Button
            label={picked.size > 1 ? `Add ${picked.size}` : "Add"}
            variant="primary"
            isDisabled={picked.size === 0}
            onClick={() => onConfirm([...picked.values()])}
          />
        </HStack>
      </HStack>
    </Dialog>
  );
}
