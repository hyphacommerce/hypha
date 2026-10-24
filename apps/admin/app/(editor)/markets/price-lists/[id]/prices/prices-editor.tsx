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
      label="Search products"
      isLabelHidden
      placeholder="Search title, handle or SKU"
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
  const phase = phaseBadge(list.phase);
  const variantCount = products.reduce((sum, p) => sum + p.variants.length, 0);

  return (
    <>
      <PriceGrid
        pageKey={pageKey}
        currency={list.currency}
        products={products}
        isReadOnly={!canWrite}
        isLoading={isNavigating}
        label={`Prices in ${list.name}`}
        onSave={(diff) => saveFixedPrices(list.id, diff)}
        renderChrome={({ isDirty, confirmLeave }) => ({
          title: (
            <>
              <IconButton
                label={`Back to ${list.name}`}
                tooltip="Back to the list"
                icon={<ArrowLeftIcon width={16} height={16} />}
                size="sm"
                variant="ghost"
                onClick={() =>
                  confirmLeave(() => router.push(`/markets/price-lists/${list.id}?tab=prices`))
                }
              />
              <h1 className="pe-name">{list.name}</h1>
              <Badge
                label={kindLabel(list.kind)}
                variant={list.kind === "sale" ? "pink" : "blue"}
              />
              <Badge label={phase.label} variant={phase.variant} />
              <span className="pe-meta">
                <span>{list.currency}</span>
                <span>{adjustmentLabel(list.adjustment)}</span>
                <span>
                  {list.fixed_price_count.toLocaleString("en")} fixed{" "}
                  {list.fixed_price_count === 1 ? "price" : "prices"}
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
                label="Category"
                isLabelHidden
                size="sm"
                placeholder="Any category"
                options={categories}
                value={filters.category ?? null}
                hasClear
                hasSearch
                isDisabled={isDirty}
                onChange={(category: string | null) => go({ category: category ?? undefined })}
                width={200}
              />
              <Switch
                label="Fixed prices only"
                size="sm"
                value={filters.fixedOnly}
                isDisabled={isDirty}
                onChange={(fixedOnly) => go({ fixedOnly })}
              />
              {isDirty && (
                <Text type="supporting" color="secondary">
                  Save or discard to change filters
                </Text>
              )}
            </div>
          ),
          tools: (
            <div className="pe-group">
              <Button
                label="Export CSV"
                size="sm"
                variant="ghost"
                // A download, not a page: the route answers with the file.
                onClick={() => window.location.assign(exportHref(list.id, filters))}
              />
              {canWrite && (
                <Button
                  label="Import CSV"
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
                  {variantCount.toLocaleString("en")} {variantCount === 1 ? "variant" : "variants"}{" "}
                  on this page
                </span>
              )}
              {!isFirstPage && (
                <Button
                  label="First page"
                  size="sm"
                  variant="ghost"
                  isDisabled={isDirty}
                  onClick={() => go({})}
                />
              )}
              {nextCursor && (
                <Button
                  label="Next page"
                  size="sm"
                  isDisabled={isDirty}
                  tooltip={isDirty ? "Save or discard your changes first" : undefined}
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
                ? "No products match these filters"
                : isFirstPage
                  ? "No products yet"
                  : "No more products"}
            </Text>
            <Text type="supporting" color="secondary">
              {filters.fixedOnly
                ? "Only variants with a fixed price in this list are shown. Turn off “Fixed prices only” to set new ones."
                : isFiltered
                  ? "Try another search or category."
                  : "Products you add show here with their prices."}
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
          aria-label="CSV file to import"
          onChange={(e) => {
            const file = e.target.files?.[0];
            e.target.value = "";
            if (!file) return;
            if (file.size > MAX_IMPORT_BYTES) {
              setFileError(
                `That file is over ${MAX_IMPORT_BYTES / 1024 / 1024} MB. Split it into smaller files.`,
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
