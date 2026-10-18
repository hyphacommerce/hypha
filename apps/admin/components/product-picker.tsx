"use client";

// Picking products (design note 0024, section 6): one dialog for every place that adds
// products to something (a category, a catalog, a fixed price). It lists products as
// soon as it opens, narrows them as you type and by status, shows each with its image,
// handle and status, and marks the ones already there so they can't be picked twice.

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { CheckboxInput } from "@astryxdesign/core/CheckboxInput";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Skeleton } from "@astryxdesign/core/Skeleton";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import { ToggleButton } from "@astryxdesign/core/ToggleButton";
import type { AdminSchemas } from "@hypha/sdk/admin";
import { MagnifyingGlassIcon, PhotoIcon } from "@heroicons/react/24/outline";
import { useEffect, useRef, useState, useTransition } from "react";

import { ProductStatusBadge } from "@/components/status";

import { type PickerProduct, pickerProducts } from "./product-picker-actions";

type Status = AdminSchemas["ProductStatus"];

const STATUSES: { value: Status; label: string }[] = [
  { value: "active", label: "Active" },
  { value: "draft", label: "Draft" },
  { value: "archived", label: "Archived" },
];

export type { PickerProduct };

function addLabel(n: number): string {
  return n > 1 ? `Add ${n} products` : "Add product";
}

/** What a search is for, to tell whether the list shown answers it. */
function searchKey(q: string, statuses: Status[]): string {
  return JSON.stringify([q, statuses]);
}

export function ProductPicker({
  title,
  description,
  confirmLabel = addLabel,
  mode = "multiple",
  isAlreadyIn,
  alreadyInLabel = "Already added",
  onConfirm,
  onClose,
}: {
  title: string;
  description?: string;
  /** The confirm button's label for `n` picked products. */
  confirmLabel?: (n: number) => string;
  /** `single` picks one product: choosing another replaces it. */
  mode?: "single" | "multiple";
  /** Products already where they'd be added: shown ticked and not pickable. */
  isAlreadyIn?: (product: PickerProduct) => boolean;
  alreadyInLabel?: string;
  /**
   * Called with the picked products; an error keeps the dialog open and shows it,
   * nothing closes it.
   */
  onConfirm: (products: PickerProduct[]) => Promise<{ error?: string } | void>;
  onClose: () => void;
}) {
  const [q, setQ] = useState("");
  const [statuses, setStatuses] = useState<Status[]>([]);
  const [items, setItems] = useState<PickerProduct[]>();
  const [next, setNext] = useState<string | null>(null);
  const [picked, setPicked] = useState<Map<string, PickerProduct>>(() => new Map());
  const [error, setError] = useState<string>();
  // The search the shown items answer: loading while it isn't the current one.
  const [shownFor, setShownFor] = useState<string>();
  const isLoading = shownFor !== searchKey(q, statuses);
  const [isLoadingMore, setIsLoadingMore] = useState(false);
  const [isSaving, startSaving] = useTransition();
  // Only the latest search's answer counts: typing fast starts several.
  const latest = useRef(0);

  useEffect(() => {
    const request = ++latest.current;
    const timer = setTimeout(
      async () => {
        const page = await pickerProducts({ q, statuses });
        if (request !== latest.current) return;
        setItems(page.items);
        setNext(page.nextCursor);
        setError(page.error);
        setShownFor(searchKey(q, statuses));
      },
      q ? 250 : 0,
    );
    return () => clearTimeout(timer);
  }, [q, statuses]);

  const loadMore = async () => {
    if (!next) return;
    const request = latest.current;
    setIsLoadingMore(true);
    let page;
    try {
      page = await pickerProducts({ q, statuses, cursor: next });
    } finally {
      setIsLoadingMore(false);
    }
    if (request !== latest.current) return;
    setItems((before) => [...(before ?? []), ...page.items]);
    setNext(page.nextCursor);
    setError(page.error);
  };

  const toggle = (product: PickerProduct) =>
    setPicked((before) => {
      const after = new Map(mode === "single" ? [] : before);
      if (before.has(product.id)) after.delete(product.id);
      else after.set(product.id, product);
      return after;
    });

  const toggleStatus = (status: Status) =>
    setStatuses((before) =>
      before.includes(status) ? before.filter((s) => s !== status) : [...before, status],
    );

  const confirm = () =>
    startSaving(async () => {
      const result = await onConfirm([...picked.values()]);
      if (result?.error) setError(result.error);
    });

  return (
    <Dialog
      isOpen
      onOpenChange={(isOpen) => !isOpen && onClose()}
      width={640}
      maxHeight="85dvh"
      purpose="form"
      padding={0}
    >
      <div style={{ padding: "16px 16px 0" }}>
        <DialogHeader
          title={title}
          subtitle={description}
          onOpenChange={(isOpen) => !isOpen && onClose()}
        />
      </div>
      <VStack gap={3} padding={4}>
        <TextInput
          label="Search products"
          isLabelHidden
          placeholder="Search by title, handle or SKU"
          startIcon={MagnifyingGlassIcon}
          value={q}
          onChange={setQ}
          hasClear
          hasAutoFocus
        />
        <HStack gap={2} vAlign="center" wrap="wrap">
          {STATUSES.map((s) => (
            <ToggleButton
              key={s.value}
              label={s.label}
              size="sm"
              isPressed={statuses.includes(s.value)}
              onPressedChange={() => toggleStatus(s.value)}
            />
          ))}
        </HStack>
        {error && <Banner status="error" title={error} />}
      </VStack>

      <div className="picker-list" aria-busy={isLoading}>
        {isLoading && !items ? (
          <VStack gap={2} padding={4}>
            {Array.from({ length: 6 }, (_, i) => (
              <Skeleton key={i} index={i} height={44} />
            ))}
          </VStack>
        ) : items && items.length === 0 ? (
          <VStack gap={1} padding={6} hAlign="center">
            <Text weight="medium">
              {q || statuses.length ? "No products match" : "No products yet"}
            </Text>
            <Text type="supporting" color="secondary">
              {q || statuses.length
                ? "Try other words, or clear the status filters."
                : "Create a product first, then add it here."}
            </Text>
          </VStack>
        ) : (
          <div style={{ opacity: isLoading ? 0.6 : 1 }}>
            {items?.map((product) => {
              const already = isAlreadyIn?.(product) ?? false;
              const isPicked = picked.has(product.id);
              return (
                // The whole row is the checkbox's hit area for the mouse; keyboards and
                // screen readers use the checkbox itself, so the row stays presentational.
                <div
                  key={product.id}
                  role="presentation"
                  className="picker-row"
                  data-disabled={already || undefined}
                  data-picked={isPicked || undefined}
                  onClick={() => !already && toggle(product)}
                >
                  {/* The checkbox toggles itself; the row mustn't toggle it back. */}
                  <div role="presentation" onClick={(e) => e.stopPropagation()}>
                    <CheckboxInput
                      label={`Pick ${product.title}`}
                      isLabelHidden
                      value={already || isPicked}
                      isDisabled={already}
                      disabledMessage={already ? alreadyInLabel : undefined}
                      onChange={() => toggle(product)}
                    />
                  </div>
                  <div className="picker-thumb">
                    {product.image ? (
                      <img src={product.image} alt="" />
                    ) : (
                      <PhotoIcon width={18} aria-hidden />
                    )}
                  </div>
                  <VStack gap={0}>
                    <Text weight="medium" maxLines={1}>
                      {product.title}
                    </Text>
                    <Text type="supporting" color="secondary" maxLines={1}>
                      /{product.handle}
                      {product.variantCount > 1 && ` · ${product.variantCount} variants`}
                      {already && ` · ${alreadyInLabel}`}
                    </Text>
                  </VStack>
                  <ProductStatusBadge status={product.status} />
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
          {picked.size === 0
            ? mode === "single"
              ? "Pick a product"
              : "Pick one or more products"
            : mode === "single"
              ? [...picked.values()][0]?.title
              : `${picked.size} selected`}
        </Text>
        <HStack gap={2}>
          <Button label="Cancel" variant="ghost" onClick={onClose} />
          <Button
            label={confirmLabel(picked.size)}
            variant="primary"
            isDisabled={picked.size === 0}
            isLoading={isSaving}
            onClick={confirm}
          />
        </HStack>
      </HStack>
    </Dialog>
  );
}
