"use client";

// A codes discount's codes: searched and filtered by batch and state on the API, added
// by hand or generated in a batch, exported as CSV, deactivated or reactivated, and
// removed (the selected ones, or a batch).

import { AlertDialog } from "@astryxdesign/core/AlertDialog";
import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { type TableColumn, pixel, proportional } from "@astryxdesign/core/Table";
import { Text } from "@astryxdesign/core/Text";
import { TextArea } from "@astryxdesign/core/TextArea";
import { TextInput } from "@astryxdesign/core/TextInput";
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { TicketIcon } from "@heroicons/react/24/outline";
import { useState, useTransition } from "react";

import { DateTime } from "@/components/date-time";
import { type FilterDef, ResourceTable } from "@/components/resource-table/resource-table";
import {
  MAX_GENERATED_LENGTH,
  MIN_GENERATED_LENGTH,
  MIN_RANDOM_CHARACTERS,
  codesFrom,
  generationProblem,
} from "@/lib/discounts/codes";
import { parseCount } from "@/lib/discounts/validate";

import { addCodes, generateCodes, removeCodes, setCodesActive } from "../actions";

type Code = AdminSchemas["DiscountCodeResponse"];

const FILTERS: FilterDef[] = [
  { kind: "search", param: "q", label: "Search", placeholder: "Search codes" },
  { kind: "text", param: "batch", label: "Batch", placeholder: "A batch's label" },
  {
    kind: "select",
    param: "active",
    label: "Status",
    options: [
      { value: "true", label: "Active" },
      { value: "false", label: "Inactive" },
    ],
  },
];

/** The most codes added in one go, and generated. */
const MAX_ADDED = 1000;
const MAX_GENERATED = 10_000;

const COLUMNS: TableColumn<Code>[] = [
  {
    key: "code",
    header: "Code",
    width: proportional(2, { minWidth: 180 }),
    renderCell: (c) => <Text weight="semibold">{c.normalized}</Text>,
  },
  {
    key: "used",
    header: "Uses",
    width: pixel(120),
    align: "end",
    renderCell: (c) => (
      <Text>
        {c.usage_limit == null
          ? c.used.toLocaleString("en")
          : `${c.used.toLocaleString("en")} of ${c.usage_limit.toLocaleString("en")}`}
      </Text>
    ),
  },
  {
    key: "active",
    header: "Status",
    width: pixel(110),
    renderCell: (c) =>
      c.active ? (
        <Badge label="Active" variant="success" />
      ) : (
        <Badge label="Inactive" variant="neutral" />
      ),
  },
  {
    key: "batch",
    header: "Batch",
    width: proportional(1, { minWidth: 120 }),
    renderCell: (c) => <Text color={c.batch ? undefined : "secondary"}>{c.batch ?? "None"}</Text>,
  },
  {
    key: "created_at",
    header: "Added",
    width: pixel(160),
    renderCell: (c) =>
      c.created_at ? (
        <Text type="supporting">
          <DateTime value={c.created_at} short />
        </Text>
      ) : null,
  },
];

/** A usage limit as typed: blank for none, else a whole number from 1. */
function limitFrom(text: string): number | null | "invalid" {
  if (text.trim() === "") return null;
  return parseCount(text) ?? "invalid";
}

function AddCodesDialog({ id, onClose }: { id: string; onClose: () => void }) {
  const [text, setText] = useState("");
  const [limit, setLimit] = useState("");
  const [batch, setBatch] = useState("");
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const codes = codesFrom(text);

  const submit = () => {
    const usageLimit = limitFrom(limit);
    if (codes.length === 0) return setError("Type at least one code.");
    if (codes.length > MAX_ADDED) return setError(`At most ${MAX_ADDED} codes at once.`);
    if (usageLimit === "invalid") return setError("Uses per code: a whole number, or blank.");
    startTransition(async () => {
      const result = await addCodes(
        id,
        codes.map((code) => ({ code, usage_limit: usageLimit })),
        batch.trim() || null,
      );
      if (result.error) setError(result.error);
      else onClose();
    });
  };

  return (
    <Dialog isOpen onOpenChange={(open) => !open && onClose()} width={520} purpose="form">
      <DialogHeader
        title="Add codes"
        subtitle="Codes are compared ignoring case: welcome10 and WELCOME10 are one code, unique in the store."
        onOpenChange={(open) => !open && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <TextArea
          label="Codes"
          description="One per line, or separated by commas."
          rows={6}
          value={text}
          onChange={setText}
        />
        <TextInput
          label="Uses per code"
          isOptional
          description="Blank: only the discount's own limits."
          value={limit}
          onChange={setLimit}
          width={200}
        />
        <TextInput
          label="Batch"
          isOptional
          description="A label to find, export or remove them by later."
          value={batch}
          onChange={setBatch}
        />
        <HStack gap={2} hAlign="end">
          <Button label="Cancel" variant="ghost" onClick={onClose} />
          <Button
            label={codes.length > 1 ? `Add ${codes.length} codes` : "Add code"}
            variant="primary"
            isLoading={isPending}
            onClick={submit}
          />
        </HStack>
      </VStack>
    </Dialog>
  );
}

function GenerateDialog({ id, onClose }: { id: string; onClose: () => void }) {
  const [count, setCount] = useState("100");
  const [prefix, setPrefix] = useState("");
  const [length, setLength] = useState("12");
  const [limit, setLimit] = useState("1");
  const [batch, setBatch] = useState("");
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();

  const submit = () => {
    const n = parseCount(count);
    const size = parseCount(length);
    const usageLimit = limitFrom(limit);
    if (n === null || n > MAX_GENERATED) {
      return setError(`How many: 1 to ${MAX_GENERATED.toLocaleString("en")}.`);
    }
    const problem = generationProblem(size ?? 0, prefix);
    if (size === null || problem) return setError(problem ?? undefined);
    if (usageLimit === "invalid") return setError("Uses per code: a whole number, or blank.");
    startTransition(async () => {
      const result = await generateCodes(id, {
        count: n,
        prefix: prefix.trim() || null,
        length: size,
        usage_limit: usageLimit,
        batch: batch.trim() || null,
      });
      if (result.error) setError(result.error);
      else onClose();
    });
  };

  return (
    <Dialog isOpen onOpenChange={(open) => !open && onClose()} width={520} purpose="form">
      <DialogHeader
        title="Generate codes"
        subtitle="Random codes without look-alike characters (no 0/O or 1/I/L), for flyers, partners or one per customer."
        onOpenChange={(open) => !open && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <HStack gap={3} wrap="wrap">
          <TextInput label="How many" value={count} onChange={setCount} width={140} />
          <TextInput
            label="Length"
            description={`${MIN_GENERATED_LENGTH} to ${MAX_GENERATED_LENGTH}, the prefix included, with at least ${MIN_RANDOM_CHARACTERS} random characters.`}
            value={length}
            onChange={setLength}
            width={140}
          />
        </HStack>
        <TextInput
          label="Prefix"
          isOptional
          placeholder="SPRING-"
          value={prefix}
          onChange={setPrefix}
          width={240}
        />
        <TextInput
          label="Uses per code"
          isOptional
          description="1 for single-use codes; blank for only the discount's own limits."
          value={limit}
          onChange={setLimit}
          width={200}
        />
        <TextInput
          label="Batch"
          isOptional
          description="A label to find, export or remove them by later."
          value={batch}
          onChange={setBatch}
        />
        <HStack gap={2} hAlign="end">
          <Button label="Cancel" variant="ghost" onClick={onClose} />
          <Button label="Generate" variant="primary" isLoading={isPending} onClick={submit} />
        </HStack>
      </VStack>
    </Dialog>
  );
}

function RemoveDialog({
  id,
  which,
  onClose,
}: {
  id: string;
  which: Which;
  onClose: (removed: boolean) => void;
}) {
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const what = describeWhich(which);
  return (
    <AlertDialog
      isOpen
      onOpenChange={(open) => !open && onClose(false)}
      title={`Remove ${what}?`}
      description={
        error ??
        "Buyers can't enter them any more, and carts holding them see them go. Orders keep their discount."
      }
      actionLabel="Remove"
      isActionLoading={isPending}
      onAction={() =>
        startTransition(async () => {
          const result = await removeCodes(id, which);
          if (result.error) setError(result.error);
          else onClose(true);
        })
      }
    />
  );
}

type Which = { codes: string[] } | { batch: string };

/** "SPRING10", "3 codes", "every code of the batch flyers". */
function describeWhich(which: Which): string {
  if ("batch" in which) return `every code of the batch ${which.batch}`;
  return which.codes.length === 1 ? (which.codes[0] ?? "") : `${which.codes.length} codes`;
}

function ActivateDialog({
  id,
  which,
  active,
  onClose,
}: {
  id: string;
  which: Which;
  active: boolean;
  onClose: (done: boolean) => void;
}) {
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const what = describeWhich(which);
  return (
    <AlertDialog
      isOpen
      onOpenChange={(open) => !open && onClose(false)}
      title={active ? `Reactivate ${what}?` : `Deactivate ${what}?`}
      description={
        error ??
        (active
          ? "Buyers can enter them again, within their limits."
          : "Carts refuse them as unknown until they're reactivated. Their uses so far stay, and orders keep their discount.")
      }
      actionLabel={active ? "Reactivate" : "Deactivate"}
      isActionLoading={isPending}
      onAction={() =>
        startTransition(async () => {
          const result = await setCodesActive(id, which, active);
          if (result.error) setError(result.error);
          else onClose(true);
        })
      }
    />
  );
}

type Open =
  | { kind: "add" }
  | { kind: "generate" }
  | { kind: "remove"; which: Which; clear?: () => void }
  | { kind: "activate"; which: Which; active: boolean; clear?: () => void };

export function CodesTable({
  id,
  page,
  canWrite,
  batch = null,
}: {
  id: string;
  /** The page of codes; absent while it loads. */
  page?: { items: Code[]; next_cursor: string | null; error?: string };
  canWrite: boolean;
  /** The batch the list is filtered by, for exporting and removing it. */
  batch?: string | null;
}) {
  const [open, setOpen] = useState<Open | null>(null);
  const exportHref = `/discounts/${id}/codes/export${batch ? `?batch=${encodeURIComponent(batch)}` : ""}`;
  return (
    <>
      <ResourceTable<Code>
        rows={page?.items}
        nextCursor={page?.next_cursor}
        isLoading={!page}
        columns={COLUMNS}
        idKey="normalized"
        isSelectable={canWrite}
        rowLabel={(c) => `Select ${c.normalized}`}
        filters={FILTERS}
        error={page?.error}
        toolbarEnd={
          <HStack gap={2}>
            <Button
              label={batch ? "Export batch" : "Export CSV"}
              variant="secondary"
              href={exportHref}
            />
            {canWrite && batch && (
              <>
                <Button
                  label="Deactivate batch"
                  variant="secondary"
                  onClick={() => setOpen({ kind: "activate", which: { batch }, active: false })}
                />
                <Button
                  label="Reactivate batch"
                  variant="secondary"
                  onClick={() => setOpen({ kind: "activate", which: { batch }, active: true })}
                />
                <Button
                  label="Remove batch"
                  variant="secondary"
                  onClick={() => setOpen({ kind: "remove", which: { batch } })}
                />
              </>
            )}
            {canWrite && (
              <Button
                label="Generate"
                variant="secondary"
                onClick={() => setOpen({ kind: "generate" })}
              />
            )}
            {canWrite && (
              <Button
                label="Add codes"
                variant="primary"
                onClick={() => setOpen({ kind: "add" })}
              />
            )}
          </HStack>
        }
        renderBulkActions={(selected, clear) => {
          const which = { codes: selected.map((c) => c.code) };
          return (
            <HStack gap={2}>
              {selected.some((c) => c.active) && (
                <Button
                  label="Deactivate"
                  variant="secondary"
                  size="sm"
                  onClick={() => setOpen({ kind: "activate", which, active: false, clear })}
                />
              )}
              {selected.some((c) => !c.active) && (
                <Button
                  label="Reactivate"
                  variant="secondary"
                  size="sm"
                  onClick={() => setOpen({ kind: "activate", which, active: true, clear })}
                />
              )}
              <Button
                label="Remove"
                variant="secondary"
                size="sm"
                onClick={() => setOpen({ kind: "remove", which, clear })}
              />
            </HStack>
          );
        }}
        empty={{
          icon: <TicketIcon width={32} />,
          title: "No codes yet",
          description: canWrite
            ? "Add the codes buyers will type, or generate a batch of random ones."
            : "Nobody has added codes to this discount.",
        }}
        noResults={{ title: "No codes match" }}
      />
      {open?.kind === "add" && <AddCodesDialog id={id} onClose={() => setOpen(null)} />}
      {open?.kind === "generate" && <GenerateDialog id={id} onClose={() => setOpen(null)} />}
      {open?.kind === "activate" && (
        <ActivateDialog
          id={id}
          which={open.which}
          active={open.active}
          onClose={(done) => {
            if (done) open.clear?.();
            setOpen(null);
          }}
        />
      )}
      {open?.kind === "remove" && (
        <RemoveDialog
          id={id}
          which={open.which}
          onClose={(removed) => {
            if (removed) open.clear?.();
            setOpen(null);
          }}
        />
      )}
    </>
  );
}
