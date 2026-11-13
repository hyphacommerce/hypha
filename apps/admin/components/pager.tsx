import { Button } from "@astryxdesign/core/Button";
import { HStack } from "@astryxdesign/core/Layout";

import type { TFor } from "@/lib/messages";

/**
 * Forward-only paging for cursor lists: back to the first page, or on to the next.
 * `query` is the list's other parameters (filters), kept on both links; `param` names
 * the cursor's, for a page with more than one list. `t` is the caller's translator, so
 * server and client pages render it alike.
 */
export function Pager({
  path,
  query,
  isFirstPage,
  nextCursor,
  param = "cursor",
  t,
}: {
  path: string;
  query: Record<string, string | undefined>;
  isFirstPage: boolean;
  nextCursor: string | null | undefined;
  param?: string;
  t: TFor<"table">;
}) {
  if (isFirstPage && !nextCursor) return null;
  const href = (cursor?: string) => {
    const params = new URLSearchParams();
    for (const [key, value] of Object.entries({ ...query, [param]: cursor })) {
      if (value) params.set(key, value);
    }
    const search = params.toString();
    return search ? `${path}?${search}` : path;
  };
  return (
    <HStack gap={2} hAlign="end">
      {!isFirstPage && <Button label={t("table.first_page")} variant="ghost" href={href()} />}
      {nextCursor && <Button label={t("table.next_page")} href={href(nextCursor)} />}
    </HStack>
  );
}
