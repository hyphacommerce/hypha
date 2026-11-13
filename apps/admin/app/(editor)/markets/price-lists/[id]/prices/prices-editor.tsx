"use client";

// A list's price editor: the grid over a page of products, the list it edits in the top
// bar, its filters in the URL, and the CSV export and import. While there are unsaved
// changes the filters and pages wait (save or discard first), and the way back asks.

import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { IconButton } from "@astryxdesign/core/IconButton";
import { Selector } from "@astryxdesign/core/Selector";
import { Switch } from "@astryxdesign/core/Switch";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import { ArrowLeftIcon, MagnifyingGlassIcon } from "@heroicons/react/24/outline";
import { useRouter } from "next/navigation";
import { useEffect, useEffectEvent, useRef, useState, useTransition } from "react";

import { saveFixedPrices } from "@/app/(admin)/markets/price-lists/actions";
import type { GridFilters } from "@/app/(admin)/markets/price-lists/grid-data";
import { PriceGrid, type PriceGridProduct } from "@/components/price-grid/price-grid";
import { MAX_IMPORT_BYTES } from "@/lib/price-import";
import { type PriceList, adjustmentLabel, kindLabel, phaseBadge } from "@/lib/price-lists";
import { useFormat } from "@/lib/use-format";
import { useT } from "@/lib/use-t";

import { exportHref, pricesHref } from "@/lib/price-editor";
import { ImportDialog } from "./import-dialog";

/** The search box: local while typing, into the URL once the user pauses. */
function SearchBox({
  value,
  isDisabled,
  onApply,
}: {
  value: string;
  isDisabled: boolean;
  onApply: (q: string) => void;
}) {
  const t = useT<"price_lists">();
  const [text, setText] = useState(value);
  const [shownValue, setShownValue] = useState(value);
  if (value !== shownValue) {
    setShownValue(value);
    setText(value);
  }
  const apply = useEffectEvent((q: string) => onApply(q));
  useEffect(() => {
    if (isDisabled || text.trim() === value.trim()) return;
    const timer = setTimeout(() => apply(text.trim()), 400);
    return () => clearTimeout(timer);
  }, [text, value, isDisabled]);
  return (
    <TextInput
      label={t("price_lists.editor_search_label")}
      isLabelHidden
      placeholder={t("price_lists.editor_search_placeholder")}
      startIcon={MagnifyingGlassIcon}
      hasClear
      size="sm"
      width={240}
      isDisabled={isDisabled}
      value={text}
      onChange={setText}
      onEnter={() => onApply(text.trim())}
    />
  );
}

export function PricesEditor({
  pageKey,
  list,
  products,
  nextCursor,
  isFirstPage,
  filters,
  error,
  categories,
  canWrite,
}: {
  /** Names the page shown (filters and cursor): a new one starts the grid afresh. */
  pageKey: string;
  list: PriceList;
  products: PriceGridProduct[];
  nextCursor: string | null;
  isFirstPage: boolean;
  filters: GridFilters;
  /** Why the API refused the filters. */
  error?: string;
  categories: { value: string; label: string }[];
  canWrite: boolean;
}) {
  const t = useT<"price_lists">();
  const format = useFormat();
  const router = useRouter();
  const [isNavigating, startNavigation] = useTransition();
  const [importFile, setImportFile] = useState<File>();
  const [fileError, setFileError] = useState<string>();
  const fileRef = useRef<HTMLInputElement>(null);

  const go = (change: Partial<GridFilters>, cursor?: string) =>
    startNavigation(() =>
      router.replace(pricesHref(list.id, { ...filters, ...change }, cursor), { scroll: false }),
    );
  const isFiltered = Boolean(filters.q || filters.category || filters.fixedOnly);
  const phase = phaseBadge(list.phase, t);
  const variantCount = products.reduce((sum, p) => sum + p.variants.length, 0);

  return (
    <>
      <PriceGrid
        pageKey={pageKey}
        currency={list.currency}
        products={products}
        isReadOnly={!canWrite}
        isLoading={isNavigating}
        label={t("price_lists.editor_grid_label", { name: list.name })}
        onSave={(diff) => saveFixedPrices(list.id, diff)}
        renderChrome={({ isDirty, confirmLeave }) => ({
          title: (
            <>
              <IconButton
                label={t("price_lists.editor_back_label", { name: list.name })}
                tooltip={t("price_lists.editor_back_tooltip")}
                icon={<ArrowLeftIcon width={16} height={16} />}
                size="sm"
                variant="ghost"
                onClick={() =>
                  confirmLeave(() => router.push(`/markets/price-lists/${list.id}?tab=prices`))
                }
              />
              <h1 className="pe-name">{list.name}</h1>
              <Badge
                label={kindLabel(t, list.kind)}
                variant={list.kind === "sale" ? "pink" : "blue"}
              />
              <Badge label={phase.label} variant={phase.variant} />
              <span className="pe-meta">
                <span>{list.currency}</span>
                <span>{adjustmentLabel(t, format, list.adjustment)}</span>
                <span>
                  {t("price_lists.fixed_prices_count", { count: list.fixed_price_count })}
                </span>
              </span>
            </>
          ),
          filters: (
            <div className="pe-group">
              <SearchBox
                value={filters.q ?? ""}
                isDisabled={isDirty}
                onApply={(q) => go({ q: q || undefined })}
              />
              <Selector
                label={t("common.category")}
                isLabelHidden
                size="sm"
                placeholder={t("price_lists.editor_any_category")}
                options={categories}
                value={filters.category ?? null}
                hasClear
                hasSearch
                isDisabled={isDirty}
                onChange={(category: string | null) => go({ category: category ?? undefined })}
                width={200}
              />
              <Switch
                label={t("price_lists.editor_fixed_only")}
                size="sm"
                value={filters.fixedOnly}
                isDisabled={isDirty}
                onChange={(fixedOnly) => go({ fixedOnly })}
              />
              {isDirty && (
                <Text type="supporting" color="secondary">
                  {t("price_lists.editor_filters_locked")}
                </Text>
              )}
            </div>
          ),
          tools: (
            <div className="pe-group">
              <Button
                label={t("price_lists.editor_export")}
                size="sm"
                variant="ghost"
                // A download, not a page: the route answers with the file.
                onClick={() => window.location.assign(exportHref(list.id, filters))}
              />
              {canWrite && (
                <Button
                  label={t("price_lists.editor_import")}
                  size="sm"
                  variant="ghost"
                  onClick={() => fileRef.current?.click()}
                />
              )}
            </div>
          ),
          pager: (
            <div className="pe-pager">
              {variantCount > 0 && (
                <span className="pe-meta">
                  {t("price_lists.editor_variants_on_page", { count: variantCount })}
                </span>
              )}
              {!isFirstPage && (
                <Button
                  label={t("price_lists.editor_first_page")}
                  size="sm"
                  variant="ghost"
                  isDisabled={isDirty}
                  onClick={() => go({})}
                />
              )}
              {nextCursor && (
                <Button
                  label={t("price_lists.editor_next_page")}
                  size="sm"
                  isDisabled={isDirty}
                  tooltip={isDirty ? t("price_lists.editor_next_page_locked") : undefined}
                  onClick={() => go({}, nextCursor)}
                />
              )}
            </div>
          ),
          banner: (error || fileError) && (
            <Banner
              status="error"
              title={error ?? fileError}
              isDismissable={!error}
              onDismiss={() => setFileError(undefined)}
            />
          ),
        })}
        empty={
          <div className="pe-empty">
            <Text weight="medium">
              {isFiltered
                ? t("price_lists.editor_empty_filtered")
                : isFirstPage
                  ? t("price_lists.editor_empty_first")
                  : t("price_lists.editor_empty_more")}
            </Text>
            <Text type="supporting" color="secondary">
              {filters.fixedOnly
                ? t("price_lists.editor_empty_fixed_only_hint")
                : isFiltered
                  ? t("price_lists.editor_empty_filtered_hint")
                  : t("price_lists.editor_empty_hint")}
            </Text>
          </div>
        }
      />
      {canWrite && (
        <input
          ref={fileRef}
          type="file"
          accept=".csv,.tsv,.txt,text/csv,text/plain"
          hidden
          aria-label={t("price_lists.editor_import_file_label")}
          onChange={(e) => {
            const file = e.target.files?.[0];
            e.target.value = "";
            if (!file) return;
            if (file.size > MAX_IMPORT_BYTES) {
              setFileError(
                t("price_lists.import_too_large", { megabytes: MAX_IMPORT_BYTES / 1024 / 1024 }),
              );
              return;
            }
            setFileError(undefined);
            setImportFile(file);
          }}
        />
      )}
      {importFile && (
        <ImportDialog
          list={list}
          file={importFile}
          onClose={(isApplied) => {
            setImportFile(undefined);
            if (isApplied) router.refresh();
          }}
        />
      )}
    </>
  );
}
