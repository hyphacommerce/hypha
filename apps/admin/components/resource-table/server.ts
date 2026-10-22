// Server-only: reading a list page's URL into an API query, and its answer into a
// page. Never import this from a "use client" file.

import type { ApiError } from "@hypha/sdk/admin";
import { forbidden, redirect } from "next/navigation";

import { describe, isMissingScope } from "@/lib/hypha";

import { CURSOR, PREV, type SearchParams, href, one, withChanges } from "./params";

/** The page's `cursor` and `limit` for the API. */
export function pageQuery(params: SearchParams): { cursor?: string; limit?: number } {
  return { cursor: one(params, CURSOR), limit: 25 };
}

/** What a list page renders: the API's page, or why the filters were refused. */
export type ListPage<T> = {
  items: T[];
  next_cursor: string | null;
  /** A refused filter (`422`: text too long, a total sort without a currency). */
  error?: string;
};

/**
 * The page of a list call. A cursor the API doesn't take any more (`400`: from before a
 * filter changed, an old version, a hand-edited URL) starts the list again with the
 * same filters; any other `400` (an unknown filter value) starts it with none. A
 * refused filter (`422`) shows its reason with an empty table, and a list the user's
 * role can't read (`403 missing_scope`) the no-access notice.
 */
export function listPage<T>(
  result: {
    data?: { items: T[]; next_cursor?: string | null };
    error?: ApiError;
    response: Response;
  },
  path: string,
  params: SearchParams,
): ListPage<T> {
  if (result.data) {
    return { items: result.data.items, next_cursor: result.data.next_cursor ?? null };
  }
  const status = result.response.status;
  if (status === 400) {
    if (result.error?.code === "invalid_cursor" && one(params, CURSOR)) {
      redirect(href(path, withChanges(params, { [CURSOR]: undefined, [PREV]: undefined })));
    }
    redirect(path);
  }
  if (status === 422 && result.error) {
    return { items: [], next_cursor: null, error: describe(result.error) };
  }
  // The user's role can't read the list: the no-access notice, as `loaded` does.
  if (isMissingScope(result)) forbidden();
  throw new Error(`${result.response.url} failed with ${status} ${result.error?.code ?? ""}`);
}

/**
 * Midnight of `date` (YYYY-MM-DD) in `timeZone`, as RFC 3339 in UTC: where a day of the
 * store's calendar starts.
 */
export function startOfDay(date: string, timeZone: string): string {
  const [y, m, d] = date.split("-").map(Number);
  const guess = Date.UTC(y, m - 1, d);
  // The zone's offset at that instant, from how it writes the time there.
  const offset = (at: number) => {
    const parts = new Intl.DateTimeFormat("en-US", {
      timeZone,
      hourCycle: "h23",
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
    }).formatToParts(new Date(at));
    const get = (type: string) => Number(parts.find((p) => p.type === type)?.value);
    return (
      Date.UTC(
        get("year"),
        get("month") - 1,
        get("day"),
        get("hour"),
        get("minute"),
        get("second"),
      ) - at
    );
  };
  let at = guess - offset(guess);
  // Once more, for a day on which the offset changes (daylight saving).
  at = guess - offset(at);
  return new Date(at).toISOString();
}

/** The day after `date` (YYYY-MM-DD). */
export function nextDay(date: string): string {
  const [y, m, d] = date.split("-").map(Number);
  return new Date(Date.UTC(y, m - 1, d + 1)).toISOString().slice(0, 10);
}
