// The price editor's addresses (`app/(editor)/markets/price-lists/[id]/prices`), for
// its page, the editor and the list's prices tab.

/** What the editor's URL narrows the grid to: `GridFilters` in grid-data.ts. */
type Filters = { q?: string; category?: string; fixedOnly: boolean };

function filterParams(filters: Filters): URLSearchParams {
  const params = new URLSearchParams();
  if (filters.q) params.set("q", filters.q);
  if (filters.category) params.set("category", filters.category);
  if (filters.fixedOnly) params.set("fixed", "1");
  return params;
}

/** The editor with `filters`, at `cursor` (the first page without). */
export function pricesHref(id: string, filters: Filters, cursor?: string): string {
  const params = filterParams(filters);
  if (cursor) params.set("cursor", cursor);
  const search = params.toString();
  return `/markets/price-lists/${id}/prices${search ? `?${search}` : ""}`;
}

/** The CSV export: the same filters, every page. */
export function exportHref(id: string, filters: Filters): string {
  const search = filterParams(filters).toString();
  return `/markets/price-lists/${id}/export${search ? `?${search}` : ""}`;
}
