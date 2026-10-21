"use client";

// Bulk actions for the lists' selection bars (design note 0024, section 6): products
// change status, channels and categories; orders are marked paid. Taking something away
// (archiving, unpublishing, leaving a category) asks first. The outcome shows in a
// banner above the list, with each row that failed and why.

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
import type { AdminSchemas } from "@hypha/sdk/admin";
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
import { canMarkPaid } from "@/lib/payments";

type Product = AdminSchemas["ProductSummaryResponse"];
type Order = AdminSchemas["OrderSummaryResponse"];
type Customer = AdminSchemas["CustomerSummaryResponse"];

/** No customer groups. */
const NO_GROUPS: Group[] = [];

/** What a bulk action did, for the banner: `verb` is past tense ("Archived"). */
export type BulkOutcome = { verb: string; result: BulkResult };

/** "3 products", "1 product". */
function count(n: number, noun: string): string {
  return `${n} ${noun}${n === 1 ? "" : "s"}`;
}

/** The banner a finished bulk action leaves above the list until it's closed. */
export function BulkOutcomeBanner({
  outcome,
  noun,
  onClose,
}: {
  outcome: BulkOutcome;
  noun: string;
  onClose: () => void;
}) {
  const { verb, result } = outcome;
  if (result.error) {
    return (
      <Banner
        status="error"
        title={`Nothing changed: ${result.error}`}
        isDismissable
        onDismiss={onClose}
      />
    );
  }
  const parts = [`${verb} ${count(result.changed, noun)}`];
  if (result.unchanged > 0) parts.push(`${result.unchanged} already so`);
  if (result.failed.length > 0) parts.push(`${result.failed.length} failed`);
  return (
    <Banner
      status={result.failed.length > 0 ? "warning" : "success"}
      title={`${parts.join(", ")}.`}
      description={
        result.failed.length > 0 ? (
          <VStack gap={0.5}>
            {result.failed.map((f) => (
              <Text key={f.id} type="supporting">
                {f.label}: {f.error}
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

const STATUSES: { value: AdminSchemas["ProductStatus"]; label: string; verb: string }[] = [
  { value: "active", label: "Set as active", verb: "Activated" },
  { value: "draft", label: "Set as draft", verb: "Moved to draft" },
  { value: "archived", label: "Archive", verb: "Archived" },
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
  const router = useRouter();
  const [isPending, startTransition] = useTransition();
  const [confirm, setConfirm] = useState<Confirm>();
  const [categoryMode, setCategoryMode] = useState<"add" | "remove">();
  const items = selected.map((p) => ({ id: p.id, label: p.title, status: p.status }));
  const n = count(selected.length, "product");

  const run = (verb: string, action: () => Promise<BulkResult>) =>
    startTransition(async () => {
      const result = await action();
      setConfirm(undefined);
      setCategoryMode(undefined);
      clear();
      onDone({ verb, result });
      router.refresh();
    });

  return (
    <>
      <DropdownMenu
        button={{ label: "Status", variant: "secondary", size: "sm", isLoading: isPending }}
        items={STATUSES.map((s) => ({
          label: s.label,
          variant: s.value === "archived" ? ("destructive" as const) : undefined,
          onClick: () => {
            const go = () => run(s.verb, () => setProductsStatus(items, s.value));
            if (s.value === "archived") {
              setConfirm({
                title: `Archive ${n}?`,
                description:
                  "Archived products leave the storefront. A draft can't be archived directly: activate it first.",
                actionLabel: "Archive",
                run: go,
              });
            } else go();
          },
        }))}
      />
      {Object.keys(channels).length > 0 && (
        <DropdownMenu
          button={{ label: "Channels", variant: "secondary", size: "sm", isLoading: isPending }}
          menuWidth={240}
          items={[
            {
              type: "section",
              title: "Publish to",
              items: Object.entries(channels).map(([id, name]) => ({
                label: name,
                onClick: () =>
                  run(`Published to ${name}:`, () => setProductsPublished(id, items, true)),
              })),
            },
            {
              type: "section",
              title: "Unpublish from",
              items: Object.entries(channels).map(([id, name]) => ({
                label: name,
                variant: "destructive" as const,
                onClick: () =>
                  setConfirm({
                    title: `Unpublish ${n} from ${name}?`,
                    description: `${name} stops showing and selling them. Other channels keep them.`,
                    actionLabel: "Unpublish",
                    run: () =>
                      run(`Unpublished from ${name}:`, () =>
                        setProductsPublished(id, items, false),
                      ),
                  }),
              })),
            },
          ]}
        />
      )}
      {Object.keys(categories).length > 0 && (
        <DropdownMenu
          button={{ label: "Categories", variant: "secondary", size: "sm", isLoading: isPending }}
          items={[
            { label: "Add to a category…", onClick: () => setCategoryMode("add") },
            {
              label: "Remove from a category…",
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
          products={n}
          isPending={isPending}
          onClose={() => setCategoryMode(undefined)}
          onPick={(id) =>
            run(
              categoryMode === "add"
                ? `Added to ${categories[id]}:`
                : `Removed from ${categories[id]}:`,
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
  products,
  isPending,
  onClose,
  onPick,
}: {
  mode: "add" | "remove";
  categories: Record<string, string>;
  products: string;
  isPending: boolean;
  onClose: () => void;
  onPick: (category: string) => void;
}) {
  const [category, setCategory] = useState<string>();
  const options = Object.entries(categories)
    .map(([value, label]) => ({ value, label }))
    .toSorted((a, b) => a.label.localeCompare(b.label));
  return (
    <Dialog isOpen onOpenChange={(open) => !open && onClose()} width={440} purpose="form">
      <DialogHeader
        title={
          mode === "add" ? `Add ${products} to a category` : `Remove ${products} from a category`
        }
        onOpenChange={(open) => !open && onClose()}
      />
      <VStack gap={4} padding={4}>
        <Selector
          label="Category"
          options={options}
          value={category}
          onChange={setCategory}
          hasSearch
          placeholder="Choose a category"
        />
        {mode === "add" && (
          <Text type="supporting" color="secondary">
            Products already in it stay as they are.
          </Text>
        )}
        <HStack gap={2} hAlign="end">
          <Button label="Cancel" variant="ghost" onClick={onClose} />
          <Button
            label={mode === "add" ? "Add" : "Remove"}
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
  const router = useRouter();
  const [isPending, startTransition] = useTransition();
  const [isConfirming, setIsConfirming] = useState(false);
  const pending = selected.filter(canMarkPaid);
  return (
    <>
      <Button
        label={`Mark as paid${pending.length !== selected.length ? ` (${pending.length})` : ""}`}
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
          title={`Mark ${count(pending.length, "order")} as paid?`}
          description="Use this for money you collected yourself: cash on delivery, a bank transfer. Paid, cancelled and other orders in the selection are left as they are."
          actionLabel="Mark as paid"
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
              onDone({ verb: "Marked paid:", result });
              router.refresh();
            })
          }
        />
      )}
    </>
  );
}

/**
 * The selection bar's actions for customers (design note 0032, CD1): add a label to, or
 * take one off, the selected customers, when the merchant has the labels field.
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
  /** The store's customer groups (design note 0032, §6). */
  groups?: Group[];
  onDone: (outcome: BulkOutcome) => void;
}) {
  const router = useRouter();
  const [isPending, startTransition] = useTransition();
  const [mode, setMode] = useState<"add" | "remove">();
  const items = selected.map((c) => ({ id: c.id, label: customerName(c) || c.email }));
  const n = count(selected.length, "customer");
  const run = (verb: string, action: () => Promise<BulkResult>) =>
    startTransition(async () => {
      const result = await action();
      setMode(undefined);
      clear();
      onDone({ verb, result });
      router.refresh();
    });
  return (
    <>
      {labels && (
        <DropdownMenu
          button={{ label: "Labels", variant: "secondary", size: "sm", isLoading: isPending }}
          items={[
            { label: "Add a label…", onClick: () => setMode("add") },
            { label: "Remove a label…", variant: "destructive", onClick: () => setMode("remove") },
          ]}
        />
      )}
      {groups.length > 0 && (
        <DropdownMenu
          button={{ label: "Groups", variant: "secondary", size: "sm", isLoading: isPending }}
          menuWidth={240}
          items={[
            {
              type: "section",
              title: "Add to",
              items: groups.map((g) => ({
                label: g.name,
                onClick: () =>
                  run(`Added to ${g.name}:`, () => setCustomersGroup(g.id, items, true)),
              })),
            },
            {
              type: "section",
              title: "Remove from",
              items: groups.map((g) => ({
                label: g.name,
                variant: "destructive" as const,
                onClick: () =>
                  run(`Removed from ${g.name}:`, () => setCustomersGroup(g.id, items, false)),
              })),
            },
          ]}
        />
      )}
      {mode && labels && (
        <LabelDialog
          mode={mode}
          definition={labels}
          customers={n}
          onClose={() => setMode(undefined)}
          onPick={(label) =>
            run(mode === "add" ? `Labelled "${label}":` : `Took "${label}" off`, () =>
              setCustomersLabel(labels.id, items, label, mode === "add"),
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
  customers,
  onClose,
  onPick,
}: {
  mode: "add" | "remove";
  definition: Definition;
  /** "3 customers". */
  customers: string;
  onClose: () => void;
  onPick: (label: string) => void;
}) {
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
  const title = mode === "add" ? `Label ${customers}` : `Remove a label from ${customers}`;
  return (
    <Dialog isOpen onOpenChange={(open) => !open && onClose()} width={480} purpose="form">
      <DialogHeader
        title={title}
        subtitle={
          mode === "add"
            ? "Each gets the label unless it has it already."
            : "Each loses the label if it has it. Nothing else changes."
        }
        onOpenChange={(open) => !open && onClose()}
      />
      <VStack gap={4} padding={4}>
        <TextInput
          label="Label"
          placeholder={mode === "add" ? "vip, wholesale, newsletter…" : "A label they have"}
          value={label}
          onChange={setLabel}
          onEnter={submit}
        />
        {inUse && inUse.length > 0 && (
          <VStack gap={1}>
            <Text type="supporting" color="secondary">
              In use
            </Text>
            <HStack gap={1} wrap="wrap">
              {inUse.slice(0, 30).map((l) => (
                <Token key={l} label={l} size="sm" onClick={() => setLabel(l)} />
              ))}
            </HStack>
          </VStack>
        )}
        <HStack gap={2} hAlign="end">
          <Button label="Cancel" variant="ghost" onClick={onClose} />
          <Button
            label={mode === "add" ? "Add label" : "Remove label"}
            variant={mode === "add" ? "primary" : "destructive"}
            isDisabled={trimmed === ""}
            onClick={submit}
          />
        </HStack>
      </VStack>
    </Dialog>
  );
}
