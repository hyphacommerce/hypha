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
import type { Format } from "@/lib/format";
import type { TFor } from "@/lib/messages";
import { useFormat } from "@/lib/use-format";
import { useT } from "@/lib/use-t";

import { addCodes, generateCodes, removeCodes, setCodesActive } from "../actions";

type Code = AdminSchemas["DiscountCodeResponse"];

type DiscountsT = TFor<"common" | "discounts">;

/** What a prefix looks like: a code, the same in every language. */
const PREFIX_EXAMPLE = "SPRING-";

const filters = (t: DiscountsT): FilterDef[] => [
  {
    kind: "search",
    param: "q",
    label: t("common.search"),
    placeholder: t("discounts.codes_search_placeholder"),
  },
  {
    kind: "text",
    param: "batch",
    label: t("discounts.codes_batch"),
    placeholder: t("discounts.codes_batch_placeholder"),
  },
  {
    kind: "select",
    param: "active",
    label: t("common.status"),
    options: [
      { value: "true", label: t("common.active") },
      { value: "false", label: t("common.inactive") },
    ],
  },
];

/** The most codes added in one go, and generated. */
const MAX_ADDED = 1000;
const MAX_GENERATED = 10_000;

const columns = (t: DiscountsT, format: Format): TableColumn<Code>[] => [
  {
    key: "code",
    header: t("common.code"),
    width: proportional(2, { minWidth: 180 }),
    renderCell: (c) => <Text weight="semibold">{c.normalized}</Text>,
  },
  {
    key: "used",
    header: t("discounts.uses"),
    width: pixel(120),
    align: "end",
    renderCell: (c) => (
      <Text>
        {c.usage_limit == null
          ? format.number(c.used)
          : t("discounts.uses_of_limit", { used: c.used, limit: c.usage_limit })}
      </Text>
    ),
  },
  {
    key: "active",
    header: t("common.status"),
    width: pixel(110),
    renderCell: (c) =>
      c.active ? (
        <Badge label={t("common.active")} variant="success" />
      ) : (
        <Badge label={t("common.inactive")} variant="neutral" />
      ),
  },
  {
    key: "batch",
    header: t("discounts.codes_batch"),
    width: proportional(1, { minWidth: 120 }),
    renderCell: (c) => <Text color={c.batch ? undefined : "secondary"}>{c.batch ?? t("common.none")}</Text>,
  },
  {
    key: "created_at",
    header: t("discounts.codes_column_added"),
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
  const t = useT<"discounts">();
  const [text, setText] = useState("");
  const [limit, setLimit] = useState("");
  const [batch, setBatch] = useState("");
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const codes = codesFrom(text);

  const submit = () => {
    const usageLimit = limitFrom(limit);
    if (codes.length === 0) return setError(t("discounts.codes_add_none"));
    if (codes.length > MAX_ADDED) {
      return setError(t("discounts.codes_add_too_many", { max: MAX_ADDED }));
    }
    if (usageLimit === "invalid") return setError(t("discounts.codes_uses_per_code_invalid"));
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
        title={t("discounts.codes_add")}
        subtitle={t("discounts.codes_add_subtitle")}
        onOpenChange={(open) => !open && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <TextArea
          label={t("discounts.codes")}
          description={t("discounts.codes_add_hint")}
          rows={6}
          value={text}
          onChange={setText}
        />
        <TextInput
          label={t("discounts.codes_uses_per_code")}
          isOptional
          description={t("discounts.codes_add_limit_hint")}
          value={limit}
          onChange={setLimit}
          width={200}
        />
        <TextInput
          label={t("discounts.codes_batch")}
          isOptional
          description={t("discounts.codes_batch_hint")}
          value={batch}
          onChange={setBatch}
        />
        <HStack gap={2} hAlign="end">
          <Button label={t("common.cancel")} variant="ghost" onClick={onClose} />
          <Button
            label={t("discounts.codes_add_submit", { count: codes.length })}
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
  const t = useT<"discounts">();
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
      return setError(t("discounts.codes_generate_count_invalid", { max: MAX_GENERATED }));
    }
    const problem = generationProblem(size ?? 0, prefix, t);
    if (size === null || problem) return setError(problem ?? undefined);
    if (usageLimit === "invalid") return setError(t("discounts.codes_uses_per_code_invalid"));
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
        title={t("discounts.codes_generate_title")}
        subtitle={t("discounts.codes_generate_subtitle")}
        onOpenChange={(open) => !open && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <HStack gap={3} wrap="wrap">
          <TextInput
            label={t("discounts.codes_generate_count")}
            value={count}
            onChange={setCount}
            width={140}
          />
          <TextInput
            label={t("discounts.codes_generate_length")}
            description={t("discounts.codes_generate_length_hint", {
              min: MIN_GENERATED_LENGTH,
              max: MAX_GENERATED_LENGTH,
              random: MIN_RANDOM_CHARACTERS,
            })}
            value={length}
            onChange={setLength}
            width={140}
          />
        </HStack>
        <TextInput
          label={t("discounts.codes_generate_prefix")}
          isOptional
          placeholder={PREFIX_EXAMPLE}
          value={prefix}
          onChange={setPrefix}
          width={240}
        />
        <TextInput
          label={t("discounts.codes_uses_per_code")}
          isOptional
          description={t("discounts.codes_generate_limit_hint")}
          value={limit}
          onChange={setLimit}
          width={200}
        />
        <TextInput
          label={t("discounts.codes_batch")}
          isOptional
          description={t("discounts.codes_batch_hint")}
          value={batch}
          onChange={setBatch}
        />
        <HStack gap={2} hAlign="end">
          <Button label={t("common.cancel")} variant="ghost" onClick={onClose} />
          <Button
            label={t("discounts.codes_generate")}
            variant="primary"
            isLoading={isPending}
            onClick={submit}
          />
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
  const t = useT<"discounts">();
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  return (
    <AlertDialog
      isOpen
      onOpenChange={(open) => !open && onClose(false)}
      title={t("discounts.codes_remove_title", whichValues(which))}
      description={error ?? t("discounts.codes_remove_description")}
      actionLabel={t("common.remove")}
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

/**
 * What a dialog's title names, as its message tells them apart: one code by itself
 * ("SPRING10"), several by their count ("3 codes"), a batch by its label ("every code of
 * the batch flyers").
 */
function whichValues(which: Which) {
  if ("batch" in which) return { which: "batch", batch: which.batch, code: "", count: 0 };
  const [code = ""] = which.codes;
  return {
    which: which.codes.length === 1 ? "one" : "many",
    batch: "",
    code,
    count: which.codes.length,
  };
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
  const t = useT<"discounts">();
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  return (
    <AlertDialog
      isOpen
      onOpenChange={(open) => !open && onClose(false)}
      title={
        active
          ? t("discounts.codes_reactivate_title", whichValues(which))
          : t("discounts.codes_deactivate_title", whichValues(which))
      }
      description={
        error ??
        (active
          ? t("discounts.codes_reactivate_description")
          : t("discounts.codes_deactivate_description"))
      }
      actionLabel={active ? t("discounts.codes_reactivate") : t("discounts.codes_deactivate")}
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
  const t = useT<"discounts">();
  const format = useFormat();
  const [open, setOpen] = useState<Open | null>(null);
  const exportHref = `/discounts/${id}/codes/export${batch ? `?batch=${encodeURIComponent(batch)}` : ""}`;
  return (
    <>
      <ResourceTable<Code>
        rows={page?.items}
        nextCursor={page?.next_cursor}
        isLoading={!page}
        columns={columns(t, format)}
        idKey="normalized"
        isSelectable={canWrite}
        rowLabel={(c) => t("discounts.codes_select", { code: c.normalized })}
        filters={filters(t)}
        error={page?.error}
        toolbarEnd={
          <HStack gap={2}>
            <Button
              label={batch ? t("discounts.codes_export_batch") : t("discounts.codes_export_csv")}
              variant="secondary"
              href={exportHref}
            />
            {canWrite && batch && (
              <>
                <Button
                  label={t("discounts.codes_deactivate_batch")}
                  variant="secondary"
                  onClick={() => setOpen({ kind: "activate", which: { batch }, active: false })}
                />
                <Button
                  label={t("discounts.codes_reactivate_batch")}
                  variant="secondary"
                  onClick={() => setOpen({ kind: "activate", which: { batch }, active: true })}
                />
                <Button
                  label={t("discounts.codes_remove_batch")}
                  variant="secondary"
                  onClick={() => setOpen({ kind: "remove", which: { batch } })}
                />
              </>
            )}
            {canWrite && (
              <Button
                label={t("discounts.codes_generate")}
                variant="secondary"
                onClick={() => setOpen({ kind: "generate" })}
              />
            )}
            {canWrite && (
              <Button
                label={t("discounts.codes_add")}
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
                  label={t("discounts.codes_deactivate")}
                  variant="secondary"
                  size="sm"
                  onClick={() => setOpen({ kind: "activate", which, active: false, clear })}
                />
              )}
              {selected.some((c) => !c.active) && (
                <Button
                  label={t("discounts.codes_reactivate")}
                  variant="secondary"
                  size="sm"
                  onClick={() => setOpen({ kind: "activate", which, active: true, clear })}
                />
              )}
              <Button
                label={t("common.remove")}
                variant="secondary"
                size="sm"
                onClick={() => setOpen({ kind: "remove", which, clear })}
              />
            </HStack>
          );
        }}
        empty={{
          icon: <TicketIcon width={32} />,
          title: t("discounts.codes_empty_title"),
          description: canWrite
            ? t("discounts.codes_empty_description")
            : t("discounts.codes_empty_description_read_only"),
        }}
        noResults={{ title: t("discounts.codes_no_results") }}
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
