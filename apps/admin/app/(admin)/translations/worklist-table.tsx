"use client";

// The worklist's table: each resource with text in the chosen state, how many of its
// fields are missing or outdated, and the way into its editor on that language.

import { Button } from "@astryxdesign/core/Button";
import { Link } from "@astryxdesign/core/Link";
import { type TableColumn, pixel, proportional } from "@astryxdesign/core/Table";
import { Text } from "@astryxdesign/core/Text";
import { LanguageIcon } from "@heroicons/react/24/outline";

import { type FilterDef, ResourceTable } from "@/components/resource-table/resource-table";
import { shortId } from "@/lib/agents";
import {
  type WorklistItem,
  type WorklistState,
  isStoreWide,
  kindLabel,
  nameKey,
} from "@/lib/translation-worklist";
import { type Kind, isKind, translateHref } from "@/lib/translations";
import { useFormat } from "@/lib/use-format";
import { useT } from "@/lib/use-t";

type WorklistRow = {
  key: string;
  kind: string;
  id: string;
  /** The resource's own name, where the page could read it. */
  name: string | undefined;
  /** Its editor on the worklist's language; none for a kind this build doesn't know. */
  href: string | undefined;
  missing: number;
  outdated: number;
  total: number;
};

const NO_NAMES: Record<string, string> = {};

export function WorklistTable({
  page,
  names = NO_NAMES,
  kinds,
  language,
  state,
  isUncounted = false,
}: {
  /** The page of resources; absent while it loads (the toolbar shows meanwhile). */
  page?: { items: WorklistItem[]; next_cursor: string | null; error?: string };
  /** Resources' names by `nameKey`. */
  names?: Record<string, string>;
  /** The kinds the role may filter by: the ones whose translations it reads. */
  kinds: readonly Kind[];
  language: string;
  state: WorklistState;
  /** The list is empty because the store's texts aren't counted yet. */
  isUncounted?: boolean;
}) {
  const t = useT<"translations">();
  const format = useFormat();

  const rows: WorklistRow[] | undefined = page?.items.map((item) => {
    const { kind, id } = item.resource;
    return {
      key: nameKey(kind, id),
      kind,
      id,
      name: names[nameKey(kind, id)],
      href: isKind(kind) ? translateHref(kind, id, language) : undefined,
      missing: item.missing,
      outdated: item.outdated,
      total: item.total,
    };
  });

  const filters: FilterDef[] = [
    {
      kind: "select",
      param: "kind",
      label: t("common.kind"),
      options: kinds.map((kind) => ({ value: kind, label: kindLabel(t, kind) })),
    },
  ];

  /** What a row is called: its name, its kind for the store's own text, else its ID. */
  const title = (row: WorklistRow) =>
    row.name ?? (isStoreWide(row.kind) ? kindLabel(t, row.kind) : shortId(row.id));
  const count = (value: number) =>
    value === 0 ? <Text color="secondary">{format.number(value)}</Text> : format.number(value);

  const columns: TableColumn<WorklistRow>[] = [
    {
      key: "name",
      header: t("common.name"),
      width: proportional(3, { minWidth: 240 }),
      renderCell: (row) =>
        row.href ? (
          <Link href={row.href} weight="semibold" color="primary">
            {title(row)}
          </Link>
        ) : (
          <Text weight="semibold">{title(row)}</Text>
        ),
    },
    {
      key: "kind",
      header: t("common.kind"),
      width: proportional(1, { minWidth: 140 }),
      renderCell: (row) => <Text>{kindLabel(t, row.kind)}</Text>,
    },
    {
      key: "missing",
      header: t("translations.worklist_column_missing"),
      width: pixel(100),
      align: "end",
      renderCell: (row) => count(row.missing),
    },
    {
      key: "outdated",
      header: t("translations.worklist_column_outdated"),
      width: pixel(100),
      align: "end",
      renderCell: (row) => count(row.outdated),
    },
    {
      key: "total",
      header: t("translations.worklist_column_total"),
      width: pixel(100),
      align: "end",
      renderCell: (row) => format.number(row.total),
    },
    {
      key: "translate",
      header: "",
      width: pixel(120),
      align: "end",
      renderCell: (row) =>
        row.href && (
          <Button
            label={t("translations.worklist_translate")}
            href={row.href}
            size="sm"
            variant="secondary"
          />
        ),
    },
  ];

  return (
    <ResourceTable<WorklistRow>
      rows={rows}
      nextCursor={page?.next_cursor}
      isLoading={!page}
      columns={columns}
      idKey="key"
      isSelectable={false}
      filters={filters}
      error={page?.error}
      empty={
        isUncounted
          ? {
              icon: <LanguageIcon width={32} />,
              title: t("translations.worklist_uncounted_title"),
              description: t("translations.worklist_uncounted_description"),
            }
          : {
              icon: <LanguageIcon width={32} />,
              title: t(`translations.worklist_empty_${state}`),
              description: t(`translations.worklist_empty_${state}_description`),
            }
      }
      noResults={
        isUncounted
          ? {
              title: t("translations.worklist_uncounted_title"),
              description: t("translations.worklist_uncounted_description"),
            }
          : { title: t(`translations.worklist_no_results_${state}`) }
      }
    />
  );
}
