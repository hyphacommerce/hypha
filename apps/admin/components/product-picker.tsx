"use client";

// Picking products: one dialog for every place that adds products to something (a
// category, a catalog, a fixed price). It lists products as soon as it opens, narrows
// them as you type and by status, shows each with its image, handle and status, and
// marks the ones already there so they can't be picked twice.

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { CheckboxInput } from "@astryxdesign/core/CheckboxInput";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Skeleton } from "@astryxdesign/core/Skeleton";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import { ToggleButton } from "@astryxdesign/core/ToggleButton";
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { MagnifyingGlassIcon, PhotoIcon } from "@heroicons/react/24/outline";
import { useEffect, useRef, useState, useTransition } from "react";

import { ProductStatusBadge, statusLabel } from "@/components/status";
import { useT } from "@/lib/use-t";

import { type PickerProduct, pickerProducts } from "./product-picker-actions";

type Status = AdminSchemas["ProductStatus"];

const STATUSES: Status[] = ["active", "draft", "archived"];

export type { PickerProduct };

/** What a search is for, to tell whether the list shown answers it. */
function searchKey(q: string, statuses: Status[]): string {
  return JSON.stringify([q, statuses]);
}

export function ProductPicker({
  title,
  description,
  confirmLabel,
  mode = "multiple",
  isAlreadyIn,
  alreadyInLabel: alreadyInLabelProp,
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
  const t = useT();
  const alreadyInLabel = alreadyInLabelProp ?? t("components.product_picker_already_added");
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
          label={t("components.product_picker_search_label")}
          isLabelHidden
          placeholder={t("components.product_picker_search_placeholder")}
          startIcon={MagnifyingGlassIcon}
          value={q}
          onChange={setQ}
          hasClear
          hasAutoFocus
        />
        <HStack gap={2} vAlign="center" wrap="wrap">
          {STATUSES.map((status) => (
            <ToggleButton
              key={status}
              label={statusLabel("product", status, t)}
              size="sm"
              isPressed={statuses.includes(status)}
              onPressedChange={() => toggleStatus(status)}
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
              {q || statuses.length
                ? t("components.product_picker_no_match_title")
                : t("components.product_picker_empty_title")}
            </Text>
            <Text type="supporting" color="secondary">
              {q || statuses.length
                ? t("components.product_picker_no_match_description")
                : t("components.product_picker_empty_description")}
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
                      label={t("components.product_picker_pick", { title: product.title })}
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
                      {[
                        `/${product.handle}`,
                        ...(product.variantCount > 1
                          ? [
                              t("common.variants_count", {
                                count: product.variantCount,
                              }),
                            ]
                          : []),
                        ...(already ? [alreadyInLabel] : []),
                      ].join(" · ")}
                    </Text>
                  </VStack>
                  <ProductStatusBadge status={product.status} t={t} />
                </div>
              );
            })}
            {next && (
              <HStack hAlign="center" padding={3}>
                <Button
                  label={t("common.load_more")}
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
              ? t("components.product_picker_hint_single")
              : t("components.product_picker_hint_multiple")
            : mode === "single"
              ? [...picked.values()][0]?.title
              : t("common.selected_count", { count: picked.size })}
        </Text>
        <HStack gap={2}>
          <Button label={t("common.cancel")} variant="ghost" onClick={onClose} />
          <Button
            label={
              confirmLabel?.(picked.size) ??
              t("components.product_picker_add", { count: picked.size })
            }
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
