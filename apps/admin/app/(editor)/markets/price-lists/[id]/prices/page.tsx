import { Button } from "@astryxdesign/core/Button";
import { Skeleton } from "@astryxdesign/core/Skeleton";
import { ArrowLeftIcon } from "@heroicons/react/24/outline";
import { redirect } from "next/navigation";
import { Suspense } from "react";

import { type GridFilters, gridPage } from "@/app/(admin)/markets/price-lists/grid-data";
import { type SearchParams, one } from "@/components/resource-table/params";
import { flatten } from "@/lib/categories";
import { describe, loaded, requireStaff, sessionClient } from "@/lib/hypha";
import { hasScope } from "@/lib/navigation";

import { pricesHref } from "@/lib/price-editor";
import { PricesEditor } from "./prices-editor";

/** The editor's filters, from the URL. */
function filtersOf(params: SearchParams): GridFilters {
  return {
    q: one(params, "q"),
    category: one(params, "category"),
    fixedOnly: one(params, "fixed") === "1",
  };
}

/**
 * A price list's prices as a full-window grid, outside the admin's frame: a page of
 * variants (`cursor`), narrowed by `q`, `category` and `fixed=1`. The frame and the way
 * back render at once; the list and its prices stream in.
 */
export default async function PriceEditorPage({
  params,
  searchParams,
}: {
  params: Promise<{ id: string }>;
  searchParams: Promise<SearchParams>;
}) {
  const [{ id }, query] = await Promise.all([params, searchParams]);
  return (
    <Suspense fallback={<EditorSkeleton id={id} />}>
      <Editor id={id} filters={filtersOf(query)} cursor={one(query, "cursor")} />
    </Suspense>
  );
}

async function Editor({
  id,
  filters,
  cursor,
}: {
  id: string;
  filters: GridFilters;
  cursor: string | undefined;
}) {
  const client = await sessionClient();
  const [list, me, categories] = await Promise.all([
    client.GET("/admin/price-lists/{id}", { params: { path: { id } } }).then(loaded),
    requireStaff(),
    client.GET("/admin/categories"),
  ]);
  const tree = categories.data?.items ?? [];
  const page = await gridPage(client, list, tree, filters, cursor);
  if (page.failure?.status === 400) {
    // An old or foreign cursor (a hand-edited URL): start the pages again.
    redirect(pricesHref(id, cursor ? filters : { fixedOnly: false }));
  }
  return (
    <PricesEditor
      pageKey={`${filters.q ?? ""}|${filters.category ?? ""}|${filters.fixedOnly}|${cursor ?? ""}`}
      list={list}
      products={page.products}
      nextCursor={page.nextCursor}
      isFirstPage={!cursor}
      filters={filters}
      error={page.failure?.error ? describe(page.failure.error) : undefined}
      categories={flatten(tree).map((row) => ({ value: row.id, label: row.path }))}
      canWrite={hasScope(me.scopes, "pricing:write")}
    />
  );
}

/** The editor's frame while the list and its prices load: the way back works already. */
function EditorSkeleton({ id }: { id: string }) {
  return (
    <div className="pe" aria-busy>
      <header className="pe-bar">
        <div className="pe-title">
          <Button
            label="Back"
            size="sm"
            variant="ghost"
            icon={<ArrowLeftIcon width={16} height={16} />}
            href={`/markets/price-lists/${id}?tab=prices`}
          />
          <Skeleton width={220} height={20} />
        </div>
      </header>
      <div className="pe-tools">
        <Skeleton width={240} height={28} />
        <Skeleton width={200} height={28} />
      </div>
      <div className="pg-scroll pe-skeleton">
        {Array.from({ length: 24 }, (_, i) => (
          <Skeleton key={i} index={i} height={20} />
        ))}
      </div>
      <footer className="pe-foot" />
    </div>
  );
}
