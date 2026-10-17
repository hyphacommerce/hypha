"use client";

// One table for every list in the admin (design note 0024, AU7): Astryx's Table with its
// sort, pagination and selection plugins, driven by the URL. The server page reads the
// URL into an API query and passes the page here; this component changes the URL when
// a header, a filter or a page changes, and the page renders again on the server.
//
// Lists the API returns whole (markets, roles: AU6) use `ClientResourceTable`, which
// sorts and filters the rows it has.

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { type DateRange, DateRangeInput } from "@astryxdesign/core/DateRangeInput";
import { EmptyState } from "@astryxdesign/core/EmptyState";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { MultiSelector } from "@astryxdesign/core/MultiSelector";
import { Selector } from "@astryxdesign/core/Selector";
import {
  Table,
  type TableColumn,
  type TablePlugin,
  TableSelectionToolbar,
  type TableSortState,
  useTablePagination,
  useTableSelection,
  useTableSelectionState,
  useTableSortable,
} from "@astryxdesign/core/Table";
import { TextInput } from "@astryxdesign/core/TextInput";
import { MagnifyingGlassIcon } from "@heroicons/react/24/outline";
import { usePathname, useRouter, useSearchParams } from "next/navigation";
import { type ReactNode, useEffect, useMemo, useState, useTransition } from "react";

import { CURSOR, PREV, encodePrevious, href, previous, withChanges } from "./params";

export type Row = Record<string, unknown>;

/** A filter above the table, and the query parameter it sets. */
export type FilterDef =
  /** Free text, applied as the user stops typing. */
  | { kind: "search" | "text"; param: string; label: string; placeholder?: string; width?: number }
  /** One value, or none. */
  | { kind: "select"; param: string; label: string; options: { value: string; label: string }[] }
  /** Any of several values: the parameter repeated. */
  | { kind: "multi"; param: string; label: string; options: { value: string; label: string }[] }
  /** Days, as `start..end` (the server turns them into instants). */
  | { kind: "dates"; param: string; label: string };

/** A sortable column and the API sort for each direction. */
export type SortDef = { asc: string; desc: string };

/**
 * One way the list can be ordered, for the Sort menu (every API sort, columns' too);
 * `unavailable` says what it needs while `sortRequires` keeps it off.
 */
export type SortOption = { value: string; label: string; unavailable?: string };

export type EmptyText = { title: string; description?: string; action?: ReactNode };

type Props<T extends Row> = {
  rows: T[];
  nextCursor: string | null;
  columns: TableColumn<T>[];
  idKey: keyof T & string;
  /** Where a row leads; the whole row is then a link (its first link cell for keyboards). */
  rowHref?: (row: T) => string;
  filters?: FilterDef[];
  /** Sortable columns by key: header clicks set `sort`. */
  sorts?: Record<string, SortDef>;
  /** Every order the list offers, for the Sort menu; absent hides it. */
  sortOptions?: SortOption[];
  /** The API's default order, when `sort` isn't in the URL. */
  defaultSort?: string;
  /** Sorts that need a filter: offered only when `requires` has a value (totals in a currency). */
  sortRequires?: Record<string, string>;
  /** Shown above the table when the API refused the filters. */
  error?: string;
  /** No rows and no filters: the list is empty. */
  empty: EmptyText & { icon?: ReactNode };
  /** No rows with filters; a Clear filters button is added. */
  noResults?: EmptyText;
  /** Bulk actions for the selected rows; absent means no checkboxes. */
  bulkActions?: (selected: T[], clear: () => void) => ReactNode;
  /** Names a row for screen readers ("Select Mug"). */
  rowLabel?: (row: T) => string;
  /** Rendered beside the filters (a view toggle, say). */
  toolbarEnd?: ReactNode;
  /** Renders the rows some other way (a grid of media) instead of the table. */
  renderRows?: (rows: T[]) => ReactNode;
};

/** Makes each body row lead to `href(row)`, unless a control inside it was used. */
function rowLinks<T extends Row>(
  rowHref: ((row: T) => string) | undefined,
  go: (url: string) => void,
): TablePlugin<T> {
  return {
    transformBodyRow: (props, item) =>
      rowHref
        ? {
            ...props,
            htmlProps: {
              ...props.htmlProps,
              style: { ...props.htmlProps.style, cursor: "pointer" },
              onClick: (event) => {
                props.htmlProps.onClick?.(event);
                const target = event.target as HTMLElement;
                if (target.closest("a, button, input, label, [role=checkbox], [role=button]")) {
                  return;
                }
                const url = rowHref(item);
                if (event.metaKey || event.ctrlKey) window.open(url, "_blank");
                else go(url);
              },
            },
          }
        : props,
  };
}

/** A text filter: local while typing, into the URL once the user pauses. */
function TextFilter({
  filter,
  value,
  apply,
}: {
  filter: Extract<FilterDef, { kind: "search" | "text" }>;
  value: string;
  apply: (value: string) => void;
}) {
  const [text, setText] = useState(value);
  useEffect(() => setText(value), [value]);
  useEffect(() => {
    if (text.trim() === value.trim()) return;
    const timer = setTimeout(() => apply(text.trim()), 400);
    return () => clearTimeout(timer);
  }, [text, value, apply]);
  return (
    <TextInput
      label={filter.label}
      isLabelHidden
      placeholder={filter.placeholder ?? filter.label}
      startIcon={filter.kind === "search" ? MagnifyingGlassIcon : undefined}
      hasClear
      size="sm"
      width={filter.width ?? (filter.kind === "search" ? 280 : 130)}
      value={text}
      onChange={setText}
      onEnter={() => apply(text.trim())}
    />
  );
}

export function ResourceTable<T extends Row>(props: Props<T>) {
  const {
    rows,
    nextCursor,
    columns,
    idKey,
    rowHref,
    filters = [],
    sorts = {},
    sortOptions,
    defaultSort,
    sortRequires = {},
    error,
    empty,
    noResults,
    bulkActions,
    rowLabel,
    toolbarEnd,
    renderRows,
  } = props;
  const router = useRouter();
  const pathname = usePathname();
  const params = useSearchParams();
  const [isPending, startTransition] = useTransition();

  const go = (changes: Record<string, string | string[] | undefined>, keepPage = false) => {
    const reset: Record<string, undefined> = keepPage ? {} : { [CURSOR]: undefined, [PREV]: undefined };
    // A sort whose filter is being cleared (a total without its currency) goes too.
    const sorted = params.get("sort");
    const needs = sorted ? sortRequires[sorted] : undefined;
    if (needs && needs in changes && !changes[needs]?.length) reset.sort = undefined;
    const url = href(pathname, withChanges(params, { ...reset, ...changes }));
    startTransition(() => router.replace(url, { scroll: false }));
  };
  const open = (url: string) => startTransition(() => router.push(url));

  // ---------- sort ----------
  const sort = params.get("sort") ?? defaultSort;
  const sortState: TableSortState<string> = useMemo(() => {
    for (const [key, def] of Object.entries(sorts)) {
      if (def.asc === sort) return [{ sortKey: key, direction: "ascending" }];
      if (def.desc === sort) return [{ sortKey: key, direction: "descending" }];
    }
    return [];
  }, [sort, sorts]);
  const sortPlugin = useTableSortable<T, string>({
    sort: sortState,
    onSortChange: (next) => {
      const entry = next[0];
      const def = entry && sorts[entry.sortKey];
      go({ sort: def ? (entry.direction === "ascending" ? def.asc : def.desc) : undefined });
    },
  });
  const isAllowed = (value: string) => {
    const needs = sortRequires[value];
    return !needs || Boolean(params.get(needs));
  };
  const sortable = columns.map((column) => {
    const def = sorts[column.key];
    return def && isAllowed(def.asc) ? { ...column, sortable: true } : column;
  });

  // ---------- pages ----------
  const stack = previous(Object.fromEntries(params.entries()));
  const current = params.get(CURSOR) ?? undefined;
  const pageNumber = stack.length + 1;
  const toPage = (page: number) => {
    if (page === pageNumber + 1 && nextCursor) {
      go({ [CURSOR]: nextCursor, [PREV]: encodePrevious([...stack, current]) }, true);
    } else if (page < pageNumber) {
      const back = stack.length - (pageNumber - page);
      // Further back than remembered: the first page.
      const cursor = back >= 0 ? stack[back] : undefined;
      go({ [CURSOR]: cursor, [PREV]: encodePrevious(back > 0 ? stack.slice(0, back) : []) }, true);
    }
  };
  const pagination = useTablePagination<T>({
    page: pageNumber,
    onPageChange: toPage,
    hasMore: Boolean(nextCursor),
    pageSize: Math.max(rows.length, 1),
    variant: "compact",
    align: "end",
    position: pageNumber === 1 && !nextCursor ? "none" : "below",
  });

  // ---------- selection ----------
  const [selectedKeys, setSelectedKeys] = useState<Set<string>>(new Set());
  const { selectionConfig, selectionState } = useTableSelectionState<T>({
    data: rows,
    idKey,
    selectedKeys,
    setSelectedKeys,
  });
  const selectionPlugin = useTableSelection<T>({ ...selectionConfig, getRowLabel: rowLabel });
  // A new page is new rows: what was selected is no longer shown.
  useEffect(() => setSelectedKeys(new Set()), [rows]);

  const links = useMemo(() => rowLinks<T>(rowHref, open), [rowHref]); // eslint-disable-line react-hooks/exhaustive-deps

  const plugins: Record<string, TablePlugin<T>> = {
    sort: sortPlugin,
    pagination,
    links,
    ...(bulkActions ? { selection: selectionPlugin } : {}),
  };

  // ---------- filters ----------
  const filterParams = filters.map((f) => f.param);
  const isFiltered = filterParams.some((p) => params.getAll(p).some((v) => v.trim() !== ""));
  const clearFilters = () => go(Object.fromEntries(filterParams.map((p) => [p, undefined])));

  const controls = filters.map((filter) => {
    switch (filter.kind) {
      case "search":
      case "text":
        return (
          <TextFilter
            key={filter.param}
            filter={filter}
            value={params.get(filter.param) ?? ""}
            apply={(value) => go({ [filter.param]: value || undefined })}
          />
        );
      case "select":
        return (
          <Selector
            key={filter.param}
            label={filter.label}
            isLabelHidden
            placeholder={filter.label}
            variant="secondary"
            size="sm"
            hasClear
            hasSearch={filter.options.length > 8}
            options={filter.options}
            value={params.get(filter.param)}
            onChange={(value: string | null) => go({ [filter.param]: value ?? undefined })}
          />
        );
      case "multi":
        return (
          <MultiSelector
            key={filter.param}
            label={filter.label}
            isLabelHidden
            placeholder={filter.label}
            variant="secondary"
            size="sm"
            triggerDisplay="labels"
            formatValue={(items) =>
              items.length === 0
                ? filter.label
                : items.length <= 2
                  ? items.map((i) => i.label).join(", ")
                  : `${filter.label}: ${items.length}`
            }
            options={filter.options}
            value={params.getAll(filter.param)}
            onChange={(values) => go({ [filter.param]: values })}
          />
        );
      case "dates": {
        const raw = params.get(filter.param);
        const [start, end] = raw?.split("..") ?? [];
        const value = start && end ? ({ start, end } as DateRange) : null;
        return (
          <DateRangeInput
            key={filter.param}
            label={filter.label}
            isLabelHidden
            placeholder={filter.label}
            size="sm"
            weekStartsOn="mon"
            value={value}
            onChange={(range) =>
              go({ [filter.param]: range ? `${range.start}..${range.end}` : undefined })
            }
          />
        );
      }
    }
  });

  const sortMenu = sortOptions && (
    <Selector
      label="Sort"
      isLabelHidden
      variant="ghost"
      size="sm"
      options={sortOptions.map((o) => ({
        value: o.value,
        label: isAllowed(o.value)
          ? `Sort: ${o.label}`
          : `Sort: ${o.label}${o.unavailable ? ` (${o.unavailable})` : ""}`,
        disabled: !isAllowed(o.value),
      }))}
      value={sort}
      onChange={(value: string) => go({ sort: value === defaultSort ? undefined : value })}
    />
  );

  const emptyState =
    isFiltered && noResults !== undefined ? (
      <EmptyState
        title={noResults.title}
        description={noResults.description}
        actions={noResults.action ?? <Button label="Clear filters" onClick={clearFilters} />}
      />
    ) : isFiltered ? (
      <EmptyState
        title="Nothing matches these filters"
        actions={<Button label="Clear filters" onClick={clearFilters} />}
      />
    ) : current ? (
      <EmptyState title="No more here" actions={<Button label="First page" onClick={() => toPage(1)} />} />
    ) : (
      <EmptyState
        icon={empty.icon}
        title={empty.title}
        description={empty.description}
        actions={empty.action}
      />
    );

  return (
    <VStack gap={3}>
      {(controls.length > 0 || sortMenu || toolbarEnd) && (
        <HStack gap={2} wrap="wrap" vAlign="center">
          {controls}
          {isFiltered && <Button label="Clear filters" variant="ghost" size="sm" onClick={clearFilters} />}
          <div style={{ flex: 1 }} />
          {sortMenu}
          {toolbarEnd}
        </HStack>
      )}
      {error && <Banner status="error" title="These filters can't be used" description={error} />}
      {bulkActions && selectionState.hasSelection && (
        <TableSelectionToolbar
          selection={selectionState}
          startContent={bulkActions(
            rows.filter((r) => selectedKeys.has(String(r[idKey]))),
            selectionState.clearSelection,
          )}
        />
      )}
      <div aria-busy={isPending} style={{ opacity: isPending ? 0.55 : 1, transition: "opacity 120ms" }}>
        {renderRows ? (
          rows.length === 0 ? (
            emptyState
          ) : (
            <VStack gap={3}>
              {renderRows(rows)}
              <Pages page={pageNumber} hasMore={Boolean(nextCursor)} toPage={toPage} />
            </VStack>
          )
        ) : (
          <Table<T>
            data={rows}
            columns={sortable}
            idKey={idKey}
            hasHover={Boolean(rowHref)}
            textOverflow="truncate"
            plugins={plugins}
            emptyState={emptyState}
          />
        )}
      </div>
    </VStack>
  );
}

/** Previous and Next for lists that don't render a table. */
function Pages({ page, hasMore, toPage }: { page: number; hasMore: boolean; toPage: (p: number) => void }) {
  if (page === 1 && !hasMore) return null;
  return (
    <HStack gap={2} hAlign="end">
      {page > 1 && <Button label="Previous" variant="ghost" onClick={() => toPage(page - 1)} />}
      {hasMore && <Button label="Next" onClick={() => toPage(page + 1)} />}
    </HStack>
  );
}

// ---------- whole lists (AU6) ----------

/** Compares rows by the text `pick` reads, for `ClientResourceTable`'s `compare`. */
export function byText<T>(pick: (row: T) => string): (a: T, b: T) => number {
  return (a, b) => pick(a).localeCompare(pick(b));
}

type ClientProps<T extends Row> = {
  rows: T[];
  columns: TableColumn<T>[];
  idKey: keyof T & string;
  rowHref?: (row: T) => string;
  /** The text a row is found by, for the search box; absent hides it. */
  searchText?: (row: T) => string;
  searchLabel?: string;
  /** How each sortable column compares. */
  compare?: Record<string, (a: T, b: T) => number>;
  empty: EmptyText & { icon?: ReactNode };
  toolbarEnd?: ReactNode;
};

/**
 * A table of a whole list (design note 0024, AU6): small lists the API returns at once
 * sort and filter here, in the browser, with the same look as the paged ones.
 */
export function ClientResourceTable<T extends Row>(props: ClientProps<T>) {
  const { rows, columns, idKey, rowHref, searchText, searchLabel, compare = {}, empty, toolbarEnd } =
    props;
  const router = useRouter();
  const [q, setQ] = useState("");
  const [sort, setSort] = useState<TableSortState<string>>([]);
  const sortPlugin = useTableSortable<T, string>({
    sort,
    onSortChange: setSort,
    allowUnsortedState: true,
  });
  const links = useMemo(() => rowLinks<T>(rowHref, (url) => router.push(url)), [rowHref, router]);

  const shown = useMemo(() => {
    const needle = q.trim().toLowerCase();
    const found = needle && searchText ? rows.filter((r) => searchText(r).toLowerCase().includes(needle)) : rows;
    const entry = sort[0];
    const cmp = entry && compare[entry.sortKey];
    if (!cmp) return found;
    const sorted = [...found].sort(cmp);
    return entry.direction === "descending" ? sorted.reverse() : sorted;
  }, [rows, q, sort, searchText, compare]);

  return (
    <VStack gap={3}>
      {(searchText || toolbarEnd) && (
        <HStack gap={2} vAlign="center">
          {searchText && (
            <TextInput
              label={searchLabel ?? "Search"}
              isLabelHidden
              placeholder={searchLabel ?? "Search"}
              startIcon={MagnifyingGlassIcon}
              hasClear
              size="sm"
              width={280}
              value={q}
              onChange={setQ}
            />
          )}
          <div style={{ flex: 1 }} />
          {toolbarEnd}
        </HStack>
      )}
      <Table<T>
        data={shown}
        columns={columns.map((c) => (compare[c.key] ? { ...c, sortable: true } : c))}
        idKey={idKey}
        hasHover={Boolean(rowHref)}
        textOverflow="truncate"
        plugins={{ sort: sortPlugin, links }}
        emptyState={
          q ? (
            <EmptyState title={`Nothing matches “${q}”`} actions={<Button label="Clear search" onClick={() => setQ("")} />} />
          ) : (
            <EmptyState icon={empty.icon} title={empty.title} description={empty.description} actions={empty.action} />
          )
        }
      />
    </VStack>
  );
}
