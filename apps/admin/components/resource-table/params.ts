// A list page's state lives in its URL: the filters, `sort`, the page's `cursor`, and
// `prev`, the cursors of the pages before it, so Previous works although the API only
// pages forward. Shared by the server page (which reads it into an API query) and the
// client table (which changes it).

/** A page's `searchParams`, as Next.js gives them. */
export type SearchParams = Record<string, string | string[] | undefined>;

/** The cursor of the page this one continues; `cursor` is the API's own parameter. */
export const CURSOR = "cursor";
/** The cursors of the pages before this one, oldest first; `-` stands for the first page. */
export const PREV = "prev";
/** How many pages back Previous remembers; older ones go through "First page". */
const PREV_DEPTH = 10;
const FIRST = "-";

/** One value of a parameter: the first if it's repeated, `undefined` if blank. */
export function one(params: SearchParams, key: string): string | undefined {
  const value = params[key];
  const first = Array.isArray(value) ? value[0] : value;
  return first?.trim() ? first : undefined;
}

/** Every value of a repeatable parameter (`status=a&status=b`), blanks dropped. */
export function many(params: SearchParams, key: string): string[] {
  const value = params[key];
  const values = Array.isArray(value) ? value : value === undefined ? [] : [value];
  return values.filter((v) => v.trim() !== "");
}

/** `one`, when it's one of `allowed` (a hand-edited value is left out). */
export function oneOf<T extends string>(
  params: SearchParams,
  key: string,
  allowed: readonly T[],
): T | undefined {
  const value = one(params, key);
  return allowed.find((a) => a === value);
}

/** `many`, keeping only the values in `allowed`. */
export function manyOf<T extends string>(
  params: SearchParams,
  key: string,
  allowed: readonly T[],
): T[] {
  return many(params, key).flatMap((v) => allowed.filter((a) => a === v));
}

/** The stack of earlier cursors; `undefined` stands for the first page. */
export function previous(params: SearchParams): (string | undefined)[] {
  const raw = one(params, PREV);
  if (!raw) return [];
  return raw.split(",").map((c) => (c === FIRST ? undefined : c));
}

export function encodePrevious(stack: (string | undefined)[]): string | undefined {
  if (stack.length === 0) return undefined;
  return stack
    .slice(-PREV_DEPTH)
    .map((c) => c ?? FIRST)
    .join(",");
}

/** `params` as a query string, with `changes` applied (`undefined` or `[]` removes). */
export function withChanges(
  params: URLSearchParams | SearchParams,
  changes: Record<string, string | string[] | undefined>,
): string {
  const next = new URLSearchParams();
  const entries: [string, string][] =
    params instanceof URLSearchParams
      ? [...params.entries()]
      : Object.entries(params).flatMap(([key, value]) =>
          (Array.isArray(value) ? value : value === undefined ? [] : [value]).map(
            (v): [string, string] => [key, v],
          ),
        );
  for (const [key, value] of entries) {
    if (!(key in changes)) next.append(key, value);
  }
  for (const [key, value] of Object.entries(changes)) {
    for (const v of Array.isArray(value) ? value : value === undefined ? [] : [value]) {
      if (v !== "") next.append(key, v);
    }
  }
  return next.toString();
}

/** `path?query`, or just `path` with no query. */
export function href(path: string, query: string): string {
  return query ? `${path}?${query}` : path;
}

/** A date range parameter, `2026-10-01..2026-10-17`. */
export function dateRange(
  params: SearchParams,
  key: string,
): { start: string; end: string } | undefined {
  const raw = one(params, key);
  const match = raw?.match(/^(\d{4}-\d{2}-\d{2})\.\.(\d{4}-\d{2}-\d{2})$/);
  return match ? { start: match[1], end: match[2] } : undefined;
}
