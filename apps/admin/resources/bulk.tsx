"use client";

// Bulk actions for the lists' selection bars: products change status, channels and
// categories; orders are marked paid. Taking something away (archiving, unpublishing,
// leaving a category) asks first. The outcome shows in a banner above the list, with
// each row that failed and why.

import { AlertDialog } from "@astryxdesign/core/AlertDialog";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { DropdownMenu } from "@astryxdesign/core/DropdownMenu";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Selector } from "@astryxdesign/core/Selector";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import { Token } from "@astryxdesign/core/Token";
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { useRouter } from "next/navigation";
import { useEffect, useState, useTransition } from "react";

import {
  type BulkResult,
  markOrdersPaid,
  setCustomersGroup,
  setCustomersLabel,
  setProductsInCategory,
  setProductsPublished,
  setProductsStatus,
} from "@/app/(admin)/bulk-actions";
import { labelsInUse } from "@/app/(admin)/customers/actions";
import type { Group } from "@/app/(admin)/customers/groups/groups";
import type { Definition } from "@/lib/custom-fields";
import { customerName } from "@/lib/format";
import type { KeyIn } from "@/lib/messages";
import { canMarkPaid } from "@/lib/payments";
import { useFormat } from "@/lib/use-format";
import { useT } from "@/lib/use-t";

type Product = AdminSchemas["ProductSummaryResponse"];
type Order = AdminSchemas["OrderSummaryResponse"];
type Customer = AdminSchemas["CustomerSummaryResponse"];

/** No customer groups. */
const NO_GROUPS: Group[] = [];

/**
 * What a bulk action did, for the banner: `summary` says what was done to how many rows,
 * already translated ("Archived 3 products").
 */
export type BulkOutcome = { summary: string; result: BulkResult };

/** What an action did to `count` rows, as a sentence without its full stop. */
type Summary = (count: number) => string;

/** The banner a finished bulk action leaves above the list until it's closed. */
export function BulkOutcomeBanner({
  outcome,
  onClose,
}: {
  outcome: BulkOutcome;
  /** No longer read: the outcome's summary names what changed. */
  noun?: string;
  onClose: () => void;
}) {
  const t = useT<"bulk">();
  const { summary, result } = outcome;
  if (result.error) {
    return (
      <Banner
        status="error"
        title={t("bulk.nothing_changed", { error: result.error })}
        isDismissable
        onDismiss={onClose}
      />
    );
  }
  const counts = { summary, unchanged: result.unchanged, failed: result.failed.length };
  const title =
    result.unchanged > 0
      ? result.failed.length > 0
        ? t("bulk.outcome_unchanged_failed", counts)
        : t("bulk.outcome_unchanged", counts)
      : result.failed.length > 0
        ? t("bulk.outcome_failed", counts)
        : t("bulk.outcome", counts);
  return (
    <Banner
      status={result.failed.length > 0 ? "warning" : "success"}
      title={title}
      description={
        result.failed.length > 0 ? (
          <VStack gap={0.5}>
            {result.failed.map((f) => (
              <Text key={f.id} type="supporting">
                {t("bulk.outcome_row_failed", { label: f.label, error: f.error })}
              </Text>
            ))}
          </VStack>
        ) : undefined
      }
      isDismissable
      onDismiss={onClose}
    />
  );
}

const STATUSES: {
  value: AdminSchemas["ProductStatus"];
  label: KeyIn<"bulk" | "common">;
  done: KeyIn<"bulk">;
}[] = [
  { value: "active", label: "bulk.products_status_active", done: "bulk.products_activated" },
  { value: "draft", label: "bulk.products_status_draft", done: "bulk.products_drafted" },
  { value: "archived", label: "common.archive", done: "bulk.products_archived" },
];

type Confirm = { title: string; description: string; actionLabel: string; run: () => void };

/** The selection bar's actions for products. */
export function ProductBulkActions({
  selected,
  clear,
  channels,
  categories,
  onDone,
}: {
  selected: Product[];
  clear: () => void;
  /** Channel names by ID. */
  channels: Record<string, string>;
  /** Category paths by ID. */
  categories: Record<string, string>;
  onDone: (outcome: BulkOutcome) => void;
}) {
  const t = useT<"bulk">();
  const router = useRouter();
  const [isPending, startTransition] = useTransition();
  const [confirm, setConfirm] = useState<Confirm>();
  const [categoryMode, setCategoryMode] = useState<"add" | "remove">();
  const items = selected.map((p) => ({ id: p.id, label: p.title, status: p.status }));
  const count = selected.length;

  const run = (summary: Summary, action: () => Promise<BulkResult>) =>
    startTransition(async () => {
      const result = await action();
      setConfirm(undefined);
      setCategoryMode(undefined);
      clear();
      onDone({ summary: summary(result.changed), result });
      router.refresh();
    });

  return (
    <>
      <DropdownMenu
        button={{
          label: t("common.status"),
          variant: "secondary",
          size: "sm",
          isLoading: isPending,
        }}
        items={STATUSES.map((s) => ({
          label: t(s.label),
          variant: s.value === "archived" ? ("destructive" as const) : undefined,
          onClick: () => {
            const go = () =>
              run(
                (changed) => t(s.done, { count: changed }),
                () => setProductsStatus(items, s.value),
              );
            if (s.value === "archived") {
              setConfirm({
                title: t("bulk.products_archive_title", { count }),
                description: t("bulk.products_archive_description"),
                actionLabel: t("common.archive"),
                run: go,
              });
            } else go();
          },
        }))}
      />
      {Object.keys(channels).length > 0 && (
        <DropdownMenu
          button={{
            label: t("common.channels"),
            variant: "secondary",
            size: "sm",
            isLoading: isPending,
          }}
          menuWidth={240}
          items={[
            {
              type: "section",
              title: t("bulk.products_publish_to"),
              items: Object.entries(channels).map(([id, name]) => ({
                label: name,
                onClick: () =>
                  run(
                    (changed) => t("bulk.products_published", { channel: name, count: changed }),
                    () => setProductsPublished(id, items, true),
                  ),
              })),
            },
            {
              type: "section",
              title: t("bulk.products_unpublish_from"),
              items: Object.entries(channels).map(([id, name]) => ({
                label: name,
                variant: "destructive" as const,
                onClick: () =>
                  setConfirm({
                    title: t("bulk.products_unpublish_title", { count, channel: name }),
                    description: t("bulk.products_unpublish_description", { channel: name }),
                    actionLabel: t("bulk.products_unpublish_action"),
                    run: () =>
                      run(
                        (changed) =>
                          t("bulk.products_unpublished", { channel: name, count: changed }),
                        () => setProductsPublished(id, items, false),
                      ),
                  }),
              })),
            },
          ]}
        />
      )}
      {Object.keys(categories).length > 0 && (
        <DropdownMenu
          button={{
            label: t("common.categories"),
            variant: "secondary",
            size: "sm",
            isLoading: isPending,
          }}
          items={[
            { label: t("bulk.products_category_add"), onClick: () => setCategoryMode("add") },
            {
              label: t("bulk.products_category_remove"),
              variant: "destructive",
              onClick: () => setCategoryMode("remove"),
            },
          ]}
        />
      )}

      {categoryMode && (
        <CategoryDialog
          mode={categoryMode}
          categories={categories}
          count={count}
          isPending={isPending}
          onClose={() => setCategoryMode(undefined)}
          onPick={(id) =>
            run(
              (changed) =>
                categoryMode === "add"
                  ? t("bulk.products_category_added", {
                      category: categories[id],
                      count: changed,
                    })
                  : t("bulk.products_category_removed", {
                      category: categories[id],
                      count: changed,
                    }),
              () => setProductsInCategory(id, items, categoryMode === "add"),
            )
          }
        />
      )}
      {confirm && (
        <AlertDialog
          isOpen
          onOpenChange={(open) => !open && setConfirm(undefined)}
          title={confirm.title}
          description={confirm.description}
          actionLabel={confirm.actionLabel}
          isActionLoading={isPending}
          onAction={confirm.run}
        />
      )}
    </>
  );
}

/** Picks the category to add the selection to, or remove it from. */
function CategoryDialog({
  mode,
  categories,
  count,
  isPending,
  onClose,
  onPick,
}: {
  mode: "add" | "remove";
  categories: Record<string, string>;
  /** How many products are selected. */
  count: number;
  isPending: boolean;
  onClose: () => void;
  onPick: (category: string) => void;
}) {
  const t = useT<"bulk">();
  const format = useFormat();
  const [category, setCategory] = useState<string>();
  const options = Object.entries(categories)
    .map(([value, label]) => ({ value, label }))
    .toSorted((a, b) => format.compare(a.label, b.label));
  return (
    <Dialog isOpen onOpenChange={(open) => !open && onClose()} width={440} purpose="form">
      <DialogHeader
        title={
          mode === "add"
            ? t("bulk.products_category_add_title", { count })
            : t("bulk.products_category_remove_title", { count })
        }
        onOpenChange={(open) => !open && onClose()}
      />
      <VStack gap={4} padding={4}>
        <Selector
          label={t("common.category")}
          options={options}
          value={category}
          onChange={setCategory}
          hasSearch
          placeholder={t("bulk.products_category_placeholder")}
        />
        {mode === "add" && (
          <Text type="supporting" color="secondary">
            {t("bulk.products_category_add_hint")}
          </Text>
        )}
        <HStack gap={2} hAlign="end">
          <Button label={t("common.cancel")} variant="ghost" onClick={onClose} />
          <Button
            label={mode === "add" ? t("common.add") : t("common.remove")}
            variant={mode === "add" ? "primary" : "destructive"}
            isDisabled={!category}
            isLoading={isPending}
            onClick={() => category && onPick(category)}
          />
        </HStack>
      </VStack>
    </Dialog>
  );
}

/** The selection bar's actions for orders: mark the unpaid ones paid. */
export function OrderBulkActions({
  selected,
  clear,
  onDone,
}: {
  selected: Order[];
  clear: () => void;
  onDone: (outcome: BulkOutcome) => void;
}) {
  const t = useT<"bulk">();
  const router = useRouter();
  const [isPending, startTransition] = useTransition();
  const [isConfirming, setIsConfirming] = useState(false);
  const pending = selected.filter(canMarkPaid);
  return (
    <>
      <Button
        label={
          pending.length === selected.length
            ? t("bulk.orders_mark_paid")
            : t("bulk.orders_mark_paid_count", { count: pending.length })
        }
        variant="secondary"
        size="sm"
        isDisabled={pending.length === 0}
        isLoading={isPending}
        onClick={() => setIsConfirming(true)}
      />
      {isConfirming && (
        <AlertDialog
          isOpen
          onOpenChange={(open) => !open && setIsConfirming(false)}
          title={t("bulk.orders_mark_paid_title", { count: pending.length })}
          description={t("bulk.orders_mark_paid_description")}
          actionLabel={t("bulk.orders_mark_paid")}
          actionVariant="primary"
          isActionLoading={isPending}
          onAction={() =>
            startTransition(async () => {
              const result = await markOrdersPaid(
                selected.map((o) => ({
                  id: o.id,
                  label: `#${o.number}`,
                  pending: canMarkPaid(o),
                })),
              );
              setIsConfirming(false);
              clear();
              onDone({
                summary: t("bulk.orders_marked_paid", { count: result.changed }),
                result,
              });
              router.refresh();
            })
          }
        />
      )}
    </>
  );
}

/**
 * The selection bar's actions for customers: add a label to, or take one off, the
 * selected customers, when the merchant has the labels field.
 */
export function CustomerBulkActions({
  selected,
  clear,
  labels,
  groups = NO_GROUPS,
  onDone,
}: {
  selected: Customer[];
  clear: () => void;
  /** The well-known `custom.tags` definition, when the merchant has it. */
  labels?: Definition;
  /** The store's customer groups. */
  groups?: Group[];
  onDone: (outcome: BulkOutcome) => void;
}) {
  const t = useT<"bulk">();
  const router = useRouter();
  const [isPending, startTransition] = useTransition();
  const [mode, setMode] = useState<"add" | "remove">();
  const items = selected.map((c) => ({ id: c.id, label: customerName(c) || c.email }));
  const run = (summary: Summary, action: () => Promise<BulkResult>) =>
    startTransition(async () => {
      const result = await action();
      setMode(undefined);
      clear();
      onDone({ summary: summary(result.changed), result });
      router.refresh();
    });
  return (
    <>
      {labels && (
        <DropdownMenu
          button={{
            label: t("bulk.customers_labels"),
            variant: "secondary",
            size: "sm",
            isLoading: isPending,
          }}
          items={[
            { label: t("bulk.customers_label_add"), onClick: () => setMode("add") },
            {
              label: t("bulk.customers_label_remove"),
              variant: "destructive",
              onClick: () => setMode("remove"),
            },
          ]}
        />
      )}
      {groups.length > 0 && (
        <DropdownMenu
          button={{
            label: t("bulk.customers_groups"),
            variant: "secondary",
            size: "sm",
            isLoading: isPending,
          }}
          menuWidth={240}
          items={[
            {
              type: "section",
              title: t("bulk.customers_group_add_to"),
              items: groups.map((g) => ({
                label: g.name,
                onClick: () =>
                  run(
                    (changed) =>
                      t("bulk.customers_group_added", { group: g.name, count: changed }),
                    () => setCustomersGroup(g.id, items, true),
                  ),
              })),
            },
            {
              type: "section",
              title: t("bulk.customers_group_remove_from"),
              items: groups.map((g) => ({
                label: g.name,
                variant: "destructive" as const,
                onClick: () =>
                  run(
                    (changed) =>
                      t("bulk.customers_group_removed", { group: g.name, count: changed }),
                    () => setCustomersGroup(g.id, items, false),
                  ),
              })),
            },
          ]}
        />
      )}
      {mode && labels && (
        <LabelDialog
          mode={mode}
          definition={labels}
          count={selected.length}
          onClose={() => setMode(undefined)}
          onPick={(label) =>
            run(
              (changed) =>
                mode === "add"
                  ? t("bulk.customers_labelled", { label, count: changed })
                  : t("bulk.customers_unlabelled", { label, count: changed }),
              () => setCustomersLabel(labels.id, items, label, mode === "add"),
            )
          }
        />
      )}
    </>
  );
}

function LabelDialog({
  mode,
  definition,
  count,
  onClose,
  onPick,
}: {
  mode: "add" | "remove";
  definition: Definition;
  /** How many customers are selected. */
  count: number;
  onClose: () => void;
  onPick: (label: string) => void;
}) {
  const t = useT<"bulk">();
  const [label, setLabel] = useState("");
  const [inUse, setInUse] = useState<string[]>();
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      const found = await labelsInUse(definition.id);
      if (!cancelled) setInUse(found);
    })();
    return () => {
      cancelled = true;
    };
  }, [definition.id]);
  const trimmed = label.trim();
  const submit = () => trimmed !== "" && onPick(trimmed);
  const title =
    mode === "add"
      ? t("bulk.customers_label_add_title", { count })
      : t("bulk.customers_label_remove_title", { count });
  return (
    <Dialog isOpen onOpenChange={(open) => !open && onClose()} width={480} purpose="form">
      <DialogHeader
        title={title}
        subtitle={
          mode === "add"
            ? t("bulk.customers_label_add_subtitle")
            : t("bulk.customers_label_remove_subtitle")
        }
        onOpenChange={(open) => !open && onClose()}
      />
      <VStack gap={4} padding={4}>
        <TextInput
          label={t("bulk.customers_label")}
          placeholder={
            mode === "add"
              ? t("bulk.customers_label_add_placeholder")
              : t("bulk.customers_label_remove_placeholder")
          }
          value={label}
          onChange={setLabel}
          onEnter={submit}
        />
        {inUse && inUse.length > 0 && (
          <VStack gap={1}>
            <Text type="supporting" color="secondary">
              {t("bulk.customers_label_in_use")}
            </Text>
            <HStack gap={1} wrap="wrap">
              {inUse.slice(0, 30).map((l) => (
                <Token key={l} label={l} size="sm" onClick={() => setLabel(l)} />
              ))}
            </HStack>
          </VStack>
        )}
        <HStack gap={2} hAlign="end">
          <Button label={t("common.cancel")} variant="ghost" onClick={onClose} />
          <Button
            label={
              mode === "add"
                ? t("bulk.customers_label_add_action")
                : t("bulk.customers_label_remove_action")
            }
            variant={mode === "add" ? "primary" : "destructive"}
            isDisabled={trimmed === ""}
            onClick={submit}
          />
        </HStack>
      </VStack>
    </Dialog>
  );
}
