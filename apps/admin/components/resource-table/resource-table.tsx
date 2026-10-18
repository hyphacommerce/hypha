"use client";

// One table for every list in the admin (design note 0024, AU7, section 6): one bordered
// card holding a toolbar (saved views, one search box, sort, and a filters button that
// opens the other filters in a row below it), the active filters as removable chips, the
// table with a shaded header and row checkboxes, and the pages in its footer. Driven by
// the URL: the server page reads it into an API query and passes the page here; this
// component changes the URL when a view, header, filter or page changes, and the page
// renders again on the server.
//
// Lists the API returns whole (markets, roles: AU6) use `ClientResourceTable`, which
// sorts and searches the rows it has, in the same card.

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import type { ISODateString } from "@astryxdesign/core/Calendar";
import { type DateRange, DateRangeInput } from "@astryxdesign/core/DateRangeInput";
import { EmptyState } from "@astryxdesign/core/EmptyState";
import { HStack } from "@astryxdesign/core/Layout";
import { MultiSelector } from "@astryxdesign/core/MultiSelector";
import { Selector } from "@astryxdesign/core/Selector";
import {
  Table,
  type TableColumn,
  type TablePlugin,
  type TableSortState,
  useTableSelection,
  useTableSelectionState,
  useTableSortable,
} from "@astryxdesign/core/Table";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import { Token } from "@astryxdesign/core/Token";
import { AdjustmentsHorizontalIcon, MagnifyingGlassIcon } from "@heroicons/react/24/outline";
import { usePathname, useRouter, useSearchParams } from "next/navigation";
import { type ReactNode, useEffect, useMemo, useState, useTransition } from "react";

import { CURSOR, PREV, encodePrevious, href, previous, withChanges } from "./params";

export type Row = Record<string, unknown>;

/** A filter, and the query parameter it sets. The `search` one sits in the toolbar. */
export type FilterDef =
  /** Free text, applied as the user stops typing. */
  | { kind: "search" | "text"; param: string; label: string; placeholder?: string; width?: number }
  /** One value, or none. */
  | { kind: "select"; param: string; label: string; options: { value: string; label: string }[] }
  /** Any of several values: the parameter repeated. */
  | { kind: "multi"; param: string; label: string; options: { value: string; label: string }[] }
  /** Days, as `start..end` (the server turns them into instants). */
  | { kind: "dates"; param: string; label: string };

/**
 * A saved view: a name for a set of filter values ("Unfulfilled"). Choosing it clears
 * the other filters; the first view is the unfiltered list ("All", no params).
 */
export type View = { label: string; params: Record<string, string[]> };

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
  /** Saved views for the selector at the start of the toolbar; absent hides it. */
  views?: View[];
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
  /** Bulk actions for the selected rows, beside the count. */
  renderBulkActions?: (selected: T[], clear: () => void) => ReactNode;
  /** Row checkboxes; on by default. */
  isSelectable?: boolean;
  /** Names a row for screen readers ("Select Mug"). */
  rowLabel?: (row: T) => string;
  /** Rendered at the end of the toolbar (a view toggle, say). */
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
                const target = event.target;
                if (
                  target instanceof Element &&
                  target.closest("a, button, input, label, [role=checkbox], [role=button]")
                ) {
                  return;
                }
                const url = rowHref(item);
                if (event.metaKey || event.ctrlKey) window.open(url, "_blank", "noopener");
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
  isWide = false,
}: {
  filter: Extract<FilterDef, { kind: "search" | "text" }>;
  value: string;
  apply: (value: string) => void;
  /** The toolbar's search box, filling the row. */
  isWide?: boolean;
}) {
  const [text, setText] = useState(value);
  // The URL changed under the box (a view, Clear all, Back): show what it says now. Set
  // while rendering, not in an effect, so the stale text never paints.
  const [shownValue, setShownValue] = useState(value);
  if (value !== shownValue) {
    setShownValue(value);
    setText(value);
  }
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
      startIcon={isWide ? MagnifyingGlassIcon : undefined}
      hasClear
      size="sm"
      width={isWide ? "100%" : (filter.width ?? 160)}
      value={text}
      onChange={setText}
      onEnter={() => apply(text.trim())}
    />
  );
}

const DAY = new Intl.DateTimeFormat("en-GB", { day: "numeric", month: "short", timeZone: "UTC" });

/** "1 Oct" from `2026-10-01`. */
function dayLabel(d?: string): string {
  return d ? DAY.format(new Date(`${d}T00:00:00Z`)) : "";
}

/** "1 Oct – 17 Oct" from `2026-10-01..2026-10-17`. */
function rangeLabel(raw: string): string {
  const [start, end] = raw.split("..");
  return `${dayLabel(start)} – ${dayLabel(end)}`;
}

/** A day as the date inputs take it; the URL may hold anything. */
function isDay(value: string | undefined): value is ISODateString {
  return value !== undefined && /^\d{4}-\d{2}-\d{2}$/.test(value);
}

// Defaults shared across renders, so a missing prop doesn't make a new object each time
// (and break the memos that depend on it).
const NO_FILTERS: FilterDef[] = [];
const NO_SORTS: Record<string, SortDef> = {};
const NO_REQUIREMENTS: Record<string, string> = {};
const NO_COMPARE: Record<string, never> = {};

/**
 * The view the URL shows: the one whose values it has exactly, among the parameters
 * views set (other filters narrow a view further, as chips).
 */
function activeView(views: View[], params: URLSearchParams): number {
  const keys = [...new Set(views.flatMap((v) => Object.keys(v.params)))];
  return views.findIndex((view) =>
    keys.every((p) => {
      const want = (view.params[p] ?? []).toSorted();
      const have = params
        .getAll(p)
        .filter((v) => v.trim() !== "")
        .toSorted();
      return want.length === have.length && want.every((v, i) => v === have[i]);
    }),
  );
}

/** The column and direction an API sort stands for, for the headers' arrows. */
function columnSort(sorts: Record<string, SortDef>, sort: string | undefined): TableSortState {
  for (const [key, def] of Object.entries(sorts)) {
    if (def.asc === sort) return [{ sortKey: key, direction: "ascending" }];
    if (def.desc === sort) return [{ sortKey: key, direction: "descending" }];
  }
  return [];
}

export function ResourceTable<T extends Row>(props: Props<T>) {
  const {
    rows,
    nextCursor,
    columns,
    idKey,
    rowHref,
    filters = NO_FILTERS,
    views,
    sorts = NO_SORTS,
    sortOptions,
    defaultSort,
    sortRequires = NO_REQUIREMENTS,
    error,
    empty,
    noResults,
    renderBulkActions,
    isSelectable = true,
    rowLabel,
    toolbarEnd,
    renderRows,
  } = props;
  const router = useRouter();
  const pathname = usePathname();
  const params = useSearchParams();
  const [isPending, startTransition] = useTransition();

  const go = (changes: Record<string, string | string[] | undefined>, keepPage = false) => {
    const reset: Record<string, undefined> = keepPage
      ? {}
      : { [CURSOR]: undefined, [PREV]: undefined };
    // A sort whose filter is being cleared (a total without its currency) goes too.
    const sorted = params.get("sort");
    const needs = sorted ? sortRequires[sorted] : undefined;
    if (needs && needs in changes && !changes[needs]?.length) reset.sort = undefined;
    const url = href(pathname, withChanges(params, { ...reset, ...changes }));
    startTransition(() => router.replace(url, { scroll: false }));
  };

  // ---------- sort ----------
  const sort = params.get("sort") ?? defaultSort;
  const sortState = useMemo(() => columnSort(sorts, sort), [sort, sorts]);
  const sortPlugin = useTableSortable<T>({
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

  // ---------- selection ----------
  const [selectedKeys, setSelectedKeys] = useState<Set<string>>(() => new Set());
  const { selectionConfig, selectionState } = useTableSelectionState<T>({
    data: rows,
    idKey,
    selectedKeys,
    setSelectedKeys,
  });
  const selectionPlugin = useTableSelection<T>({ ...selectionConfig, getRowLabel: rowLabel });
  // A new page is new rows: what was selected is no longer shown. Cleared while
  // rendering, so the old selection never paints over the new rows.
  const [selectionRows, setSelectionRows] = useState(rows);
  if (rows !== selectionRows) {
    setSelectionRows(rows);
    setSelectedKeys(new Set());
  }

  const links = useMemo(
    () => rowLinks<T>(rowHref, (url) => startTransition(() => router.push(url))),
    [rowHref, router],
  );

  const plugins: Record<string, TablePlugin<T>> = {
    sort: sortPlugin,
    links,
    ...(isSelectable || renderBulkActions ? { selection: selectionPlugin } : {}),
  };

  // ---------- filters ----------
  const search = filters.find((f) => f.kind === "search");
  const others = filters.filter((f) => f !== search);
  const filterParams = filters.map((f) => f.param);
  const valuesOf = (param: string) => params.getAll(param).filter((v) => v.trim() !== "");
  const isFiltered = filterParams.some((p) => valuesOf(p).length > 0);
  const clearFilters = () => go(Object.fromEntries(filterParams.map((p) => [p, undefined])));
  const viewIndex = views ? activeView(views, params) : -1;
  const shownView = views?.[viewIndex];
  // Filters beyond what the shown view stands for.
  const activeCount = others.filter(
    (f) => valuesOf(f.param).length > 0 && !shownView?.params[f.param],
  ).length;
  const [isFilterRowOpen, setFilterRowOpen] = useState(false);

  const control = (filter: FilterDef) => {
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
                  ? `${filter.label}: ${items.map((i) => i.label).join(", ")}`
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
        const value: DateRange | null = isDay(start) && isDay(end) ? { start, end } : null;
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
  };

  // The active filters as chips, but not the ones the shown view stands for.
  const chips = others.flatMap((filter) => {
    const values = valuesOf(filter.param);
    if (values.length === 0) return [];
    if (shownView?.params[filter.param]) return [];
    const label = (v: string) =>
      filter.kind === "select" || filter.kind === "multi"
        ? (filter.options.find((o) => o.value === v)?.label ?? v)
        : filter.kind === "dates"
          ? rangeLabel(v)
          : v;
    return [
      <Token
        key={filter.param}
        size="sm"
        label={`${filter.label}: ${values.map(label).join(", ")}`}
        onRemove={() => go({ [filter.param]: undefined })}
      />,
    ];
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
      <EmptyState
        title="No more here"
        actions={<Button label="First page" onClick={() => toPage(1)} />}
      />
    ) : (
      <EmptyState
        icon={empty.icon}
        title={empty.title}
        description={empty.description}
        actions={empty.action}
      />
    );

  const selected = rows.filter((r) => selectedKeys.has(String(r[idKey])));
  const hasToolbar = Boolean(views || search || sortMenu || others.length > 0 || toolbarEnd);

  return (
    <div className="rt" aria-busy={isPending} data-pending={isPending || undefined}>
      {hasToolbar && (
        <div className="rt-toolbar">
          {views && (
            <>
              <Selector
                label="View"
                isLabelHidden
                variant="ghost"
                size="sm"
                options={[
                  ...views.map((v, i) => ({ value: String(i), label: v.label })),
                  ...(viewIndex < 0 ? [{ value: "custom", label: "Custom" }] : []),
                ]}
                value={viewIndex < 0 ? "custom" : String(viewIndex)}
                onChange={(value: string) => {
                  const view = views[Number(value)];
                  if (!view) return;
                  // A view replaces the filters, but keeps what's being searched for.
                  go({
                    ...Object.fromEntries(
                      filterParams.filter((p) => p !== search?.param).map((p) => [p, undefined]),
                    ),
                    ...view.params,
                  });
                }}
              />
              <span className="rt-divider" aria-hidden />
            </>
          )}
          <div className="rt-search">
            {search && search.kind === "search" ? (
              <TextFilter
                filter={search}
                isWide
                value={params.get(search.param) ?? ""}
                apply={(value) => go({ [search.param]: value || undefined })}
              />
            ) : null}
          </div>
          {sortMenu}
          {others.length > 0 && (
            <Button
              label={activeCount ? `Filters (${activeCount})` : "Filters"}
              variant={isFilterRowOpen || activeCount ? "secondary" : "ghost"}
              size="sm"
              icon={<AdjustmentsHorizontalIcon width={16} aria-hidden />}
              isIconOnly={!activeCount}
              tooltip="Filters"
              onClick={() => setFilterRowOpen(!isFilterRowOpen)}
            />
          )}
          {toolbarEnd}
        </div>
      )}

      {isFilterRowOpen && others.length > 0 && (
        <div className="rt-filters">
          {others.map(control)}
          {isFiltered && (
            <Button label="Clear all" variant="ghost" size="sm" onClick={clearFilters} />
          )}
        </div>
      )}
      {!isFilterRowOpen && chips.length > 0 && (
        <div className="rt-chips">
          {chips}
          <Button label="Clear all" variant="ghost" size="sm" onClick={clearFilters} />
        </div>
      )}

      {error && (
        <div className="rt-banner">
          <Banner status="error" title="These filters can't be used" description={error} />
        </div>
      )}

      {selectionState.hasSelection && (
        <div className="rt-selection">
          <Text weight="medium">{selected.length} selected</Text>
          {renderBulkActions?.(selected, selectionState.clearSelection)}
          <div style={{ flex: 1 }} />
          <Button label="Clear" variant="ghost" size="sm" onClick={selectionState.clearSelection} />
        </div>
      )}

      <div className="rt-body">
        {renderRows ? (
          rows.length === 0 ? (
            <div className="rt-empty">{emptyState}</div>
          ) : (
            <div className="rt-grid">{renderRows(rows)}</div>
          )
        ) : (
          <Table<T>
            data={rows}
            columns={sortable}
            idKey={idKey}
            density="compact"
            hasHover={Boolean(rowHref)}
            textOverflow="truncate"
            plugins={plugins}
            emptyState={emptyState}
          />
        )}
      </div>

      <Pages page={pageNumber} count={rows.length} hasMore={Boolean(nextCursor)} toPage={toPage} />
    </div>
  );
}

/** The card's footer: where in the list this page is, Previous and Next. */
function Pages({
  page,
  count,
  hasMore,
  toPage,
}: {
  page: number;
  count: number;
  hasMore: boolean;
  toPage: (p: number) => void;
}) {
  if (page === 1 && !hasMore) return null;
  return (
    <div className="rt-footer">
      <Text type="supporting" color="secondary">
        Page {page} · {count} shown
      </Text>
      <HStack gap={2}>
        <Button
          label="Previous"
          variant="secondary"
          size="sm"
          isDisabled={page === 1}
          onClick={() => toPage(page - 1)}
        />
        <Button
          label="Next"
          variant="secondary"
          size="sm"
          isDisabled={!hasMore}
          onClick={() => toPage(page + 1)}
        />
      </HStack>
    </div>
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
 * sort and filter here, in the browser, in the same card as the paged ones.
 */
export function ClientResourceTable<T extends Row>(props: ClientProps<T>) {
  const {
    rows,
    columns,
    idKey,
    rowHref,
    searchText,
    searchLabel,
    compare = NO_COMPARE,
    empty,
    toolbarEnd,
  } = props;
  const router = useRouter();
  const [q, setQ] = useState("");
  const [sort, setSort] = useState<TableSortState>([]);
  const sortPlugin = useTableSortable<T>({
    sort,
    onSortChange: setSort,
    allowUnsortedState: true,
  });
  const links = useMemo(() => rowLinks<T>(rowHref, (url) => router.push(url)), [rowHref, router]);

  const shown = useMemo(() => {
    const needle = q.trim().toLowerCase();
    const found =
      needle && searchText
        ? rows.filter((r) => searchText(r).toLowerCase().includes(needle))
        : rows;
    const entry = sort[0];
    const cmp = entry && compare[entry.sortKey];
    if (!cmp) return found;
    const sorted = found.toSorted(cmp);
    return entry.direction === "descending" ? sorted.toReversed() : sorted;
  }, [rows, q, sort, searchText, compare]);

  return (
    <div className="rt">
      {(searchText || toolbarEnd) && (
        <div className="rt-toolbar">
          <div className="rt-search">
            {searchText && (
              <TextInput
                label={searchLabel ?? "Search"}
                isLabelHidden
                placeholder={searchLabel ?? "Search"}
                startIcon={MagnifyingGlassIcon}
                hasClear
                size="sm"
                width="100%"
                value={q}
                onChange={setQ}
              />
            )}
          </div>
          {toolbarEnd}
        </div>
      )}
      <div className="rt-body">
        <Table<T>
          data={shown}
          columns={columns.map((c) => (compare[c.key] ? { ...c, sortable: true } : c))}
          idKey={idKey}
          density="compact"
          hasHover={Boolean(rowHref)}
          textOverflow="truncate"
          plugins={{ sort: sortPlugin, links }}
          emptyState={
            q ? (
              <EmptyState
                title={`Nothing matches “${q}”`}
                actions={<Button label="Clear search" onClick={() => setQ("")} />}
              />
            ) : (
              <EmptyState
                icon={empty.icon}
                title={empty.title}
                description={empty.description}
                actions={empty.action}
              />
            )
          }
        />
      </div>
    </div>
  );
}
