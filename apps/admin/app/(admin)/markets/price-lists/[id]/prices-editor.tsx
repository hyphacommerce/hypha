"use client";

// A list's prices tab: the grid over a page of products, its filters in the URL, and the
// CSV export and import. Changing a filter or the page drops unsaved changes, like
// leaving the page.

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { EmptyState } from "@astryxdesign/core/EmptyState";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Selector } from "@astryxdesign/core/Selector";
import { Switch } from "@astryxdesign/core/Switch";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import { MagnifyingGlassIcon, TagIcon } from "@heroicons/react/24/outline";
import { useRouter } from "next/navigation";
import { useEffect, useEffectEvent, useRef, useState, useTransition } from "react";

import { PriceGrid, type PriceGridProduct } from "@/components/price-grid/price-grid";
import { MAX_IMPORT_BYTES } from "@/lib/price-import";
import type { PriceList } from "@/lib/price-lists";

import { saveFixedPrices } from "../actions";
import type { GridFilters } from "../grid-data";
import { ImportDialog } from "./import-dialog";

/** The prices tab's URL with `filters`, on its first page. */
function pricesHref(id: string, filters: GridFilters, cursor?: string): string {
  const params = new URLSearchParams({ tab: "prices" });
  if (filters.q) params.set("q", filters.q);
  if (filters.category) params.set("category", filters.category);
  if (filters.fixedOnly) params.set("fixed", "1");
  if (cursor) params.set("cursor", cursor);
  return `/markets/price-lists/${id}?${params}`;
}

/** The export's URL: the same filters, every page. */
function exportHref(id: string, filters: GridFilters): string {
  const params = new URLSearchParams();
  if (filters.q) params.set("q", filters.q);
  if (filters.category) params.set("category", filters.category);
  if (filters.fixedOnly) params.set("fixed", "1");
  const search = params.toString();
  return `/markets/price-lists/${id}/export${search ? `?${search}` : ""}`;
}

/** The search box: local while typing, into the URL once the user pauses. */
function SearchBox({ value, onApply }: { value: string; onApply: (q: string) => void }) {
  const [text, setText] = useState(value);
  const [shownValue, setShownValue] = useState(value);
  if (value !== shownValue) {
    setShownValue(value);
    setText(value);
  }
  const apply = useEffectEvent((q: string) => onApply(q));
  useEffect(() => {
    if (text.trim() === value.trim()) return;
    const timer = setTimeout(() => apply(text.trim()), 400);
    return () => clearTimeout(timer);
  }, [text, value]);
  return (
    <TextInput
      label="Search products"
      isLabelHidden
      placeholder="Search by title, handle or SKU"
      startIcon={MagnifyingGlassIcon}
      hasClear
      size="sm"
      width={260}
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

  const toolbar = (
    <HStack gap={2} vAlign="center" wrap="wrap">
      <SearchBox value={filters.q ?? ""} onApply={(q) => go({ q: q || undefined })} />
      <Selector
        label="Category"
        isLabelHidden
        size="sm"
        placeholder="Any category"
        options={categories}
        value={filters.category ?? null}
        hasClear
        hasSearch
        onChange={(category: string | null) => go({ category: category ?? undefined })}
        width={200}
      />
      <Switch
        label="Fixed prices only"
        size="sm"
        value={filters.fixedOnly}
        onChange={(fixedOnly) => go({ fixedOnly })}
      />
      <Button
        label="Export CSV"
        size="sm"
        variant="ghost"
        // A download, not a page: the route answers with the file.
        onClick={() => window.location.assign(exportHref(list.id, filters))}
      />
      {canWrite && (
        <>
          <Button
            label="Import CSV"
            size="sm"
            variant="ghost"
            onClick={() => fileRef.current?.click()}
          />
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
        </>
      )}
    </HStack>
  );

  return (
    <VStack gap={4}>
      <Text type="supporting" color="secondary">
        Base prices are each variant&apos;s own; the list price is what this list charges, greyed
        when it comes from the adjustment (worked out here from the base price). Type a fixed price
        to set one, clear it to go back to the adjustment. Unsaved changes are lost when you change
        the filters or the page.
      </Text>
      {error && <Banner status="error" title={error} />}
      {fileError && <Banner status="error" title={fileError} />}
      {/* Above the grid, not in its toolbar: the grid starts afresh on each page, and the
          search box would lose its focus and text with it. */}
      {toolbar}
      <div
        className="pending-content"
        aria-busy={isNavigating}
        data-pending={isNavigating || undefined}
      >
        {products.length === 0 ? (
          <VStack gap={3}>
            <EmptyState
              icon={<TagIcon width={32} />}
              title={
                isFiltered
                  ? "No products match these filters"
                  : isFirstPage
                    ? "No products yet"
                    : "No more products"
              }
              description={
                filters.fixedOnly
                  ? "Only variants with a fixed price in this list are shown."
                  : undefined
              }
            />
          </VStack>
        ) : (
          <PriceGrid
            key={pageKey}
            currency={list.currency}
            products={products}
            isReadOnly={!canWrite}
            label={`Prices in ${list.name}`}
            onSave={(diff) => saveFixedPrices(list.id, diff)}
          />
        )}
      </div>
      {(!isFirstPage || nextCursor) && (
        <HStack gap={2} hAlign="end">
          {!isFirstPage && <Button label="First page" variant="ghost" onClick={() => go({})} />}
          {nextCursor && <Button label="Next page" onClick={() => go({}, nextCursor)} />}
        </HStack>
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
    </VStack>
  );
}
