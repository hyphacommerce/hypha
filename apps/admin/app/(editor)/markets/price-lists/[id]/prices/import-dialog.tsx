"use client";

// Importing a CSV file into a list: the browser sends the file straight to storage (an
// upload; it never passes through the admin's server), the server reads it from there,
// matches its rows and says what they'd do, nothing written; applying runs the preview's
// steps one by one, each reading the file again, with the progress shown, and lists any
// row the API refused.

import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { ProgressBar } from "@astryxdesign/core/ProgressBar";
import { Skeleton } from "@astryxdesign/core/Skeleton";
import {
  Table,
  TableBody,
  TableCell,
  TableHeader,
  TableHeaderCell,
  TableRow,
} from "@astryxdesign/core/Table";
import { Text } from "@astryxdesign/core/Text";
import { useEffect, useEffectEvent, useState } from "react";

import type { PriceList } from "@/lib/price-lists";
import { sendToStorage } from "@/lib/storage-upload";
import { useT } from "@/lib/use-t";

import {
  type ImportPreviewResult,
  type ImportStepResult,
  applyPriceImport,
  discardPriceImport,
  previewPriceImport,
  startPriceImport,
} from "@/app/(admin)/markets/price-lists/actions";

/**
 * A row's status as a badge: its colour here, its label `price_lists.import_status_<status>`
 * and its count `price_lists.import_count_<status>`.
 */
const STATUS_VARIANTS = {
  changed: "info",
  unchanged: "neutral",
  unmatched: "warning",
  invalid: "error",
} as const;

/** The column rows are matched by first: a name in the file, the same in every language. */
const ID_COLUMN = "variant_id";

/** A row (or a whole step's lines) that applying couldn't write. */
type Failure = { key: string; where: string; error: string };

type Phase =
  | { kind: "uploading" }
  | { kind: "previewing" }
  | { kind: "preview"; preview: ImportPreviewResult; upload: string }
  | { kind: "applying"; done: number; total: number }
  | { kind: "applied"; failed: Failure[]; failedCount: number; total: number };

/**
 * Sends `file` to storage and has the server preview it: the upload's ID and the
 * preview, or why not.
 */
async function uploadAndPreview(
  list: string,
  file: File,
  onUploaded: (upload: string) => boolean,
): Promise<
  { upload: string; preview: ImportPreviewResult } | { error: string } | "not_stored" | null
> {
  const started = await startPriceImport({ filename: file.name, byte_size: file.size });
  if ("error" in started) return started;
  if (!onUploaded(started.id)) return null;
  if (!(await sendToStorage(started.upload, file))) {
    return "not_stored";
  }
  return { upload: started.id, preview: await previewPriceImport({ list, upload: started.id }) };
}

export function ImportDialog({
  list,
  file,
  onClose,
}: {
  list: PriceList;
  /** The CSV file chosen, at most `MAX_IMPORT_BYTES`. */
  file: File;
  /** `isApplied`: something was written, so the page's prices are stale. */
  onClose: (isApplied: boolean) => void;
}) {
  const t = useT<"price_lists">();
  const [phase, setPhase] = useState<Phase>({ kind: "uploading" });
  const [error, setError] = useState<string>();

  // Said when it happens, in the language of that moment: the effect below owns the
  // upload and must not run again because the translator changed.
  const report = useEffectEvent((why: "not_stored" | "check_failed") =>
    setError(t(`price_lists.import_${why}`)),
  );

  // The dialog owns its upload: deleted when it closes, applied or not.
  useEffect(() => {
    let isCurrent = true;
    let upload: string | undefined;
    const uploaded = (id: string) => {
      upload = id;
      if (!isCurrent) void discardPriceImport(id);
      else setPhase({ kind: "previewing" });
      return isCurrent;
    };
    const run = async () => {
      try {
        const result = await uploadAndPreview(list.id, file, uploaded);
        if (!isCurrent || !result) return;
        if (result === "not_stored") report(result);
        else if ("error" in result) setError(result.error);
        else setPhase({ kind: "preview", ...result });
      } catch {
        if (isCurrent) report("check_failed");
      }
    };
    void run();
    return () => {
      isCurrent = false;
      if (upload) void discardPriceImport(upload);
    };
  }, [list.id, file]);

  const apply = async (preview: ImportPreviewResult, upload: string) => {
    const total = preview.upserts + preview.clears;
    const failed: Failure[] = [];
    let failedCount = 0;
    let done = 0;
    setPhase({ kind: "applying", done, total });
    // One step at a time, on purpose: the progress bar moves with each.
    for (const step of preview.steps) {
      let result: ImportStepResult;
      try {
        result = await applyPriceImport({ list: list.id, upload, step });
      } catch {
        result = { failed: [], error: t("price_lists.import_step_failed") };
      }
      if (result.error) {
        const where = step.from === step.to ? `${step.from}` : `${step.from}–${step.to}`;
        failed.push({ key: `lines-${step.from}`, where, error: result.error });
        failedCount += step.changes;
      }
      for (const f of result.failed) {
        failed.push({ key: f.variant, where: f.line ? `${f.line}` : f.variant, error: f.error });
      }
      failedCount += result.failed.length;
      done += step.changes;
      setPhase({ kind: "applying", done, total });
    }
    setPhase({ kind: "applied", failed, failedCount, total });
  };

  // Not while applying: its steps need the upload the dialog deletes when it closes.
  const close = () => phase.kind !== "applying" && onClose(phase.kind === "applied");
  const preview = phase.kind === "preview" ? phase.preview : undefined;
  const changes = preview ? preview.upserts + preview.clears : 0;

  return (
    <Dialog isOpen onOpenChange={(isOpen) => !isOpen && close()} width={820} purpose="form">
      <DialogHeader
        title={t("price_lists.import_title")}
        subtitle={t("price_lists.import_subtitle", { column: ID_COLUMN })}
        onOpenChange={(isOpen) => !isOpen && close()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        {(phase.kind === "uploading" || phase.kind === "previewing") && !error && (
          <VStack gap={2}>
            <Text color="secondary">
              {phase.kind === "uploading"
                ? t("price_lists.import_uploading")
                : t("price_lists.import_reading")}
            </Text>
            <Skeleton height={120} />
          </VStack>
        )}
        {preview?.error && <Banner status="error" title={preview.error} />}
        {preview && !preview.error && (
          <>
            <HStack gap={2} wrap="wrap">
              {(["changed", "unchanged", "unmatched", "invalid"] as const).map((status) => (
                <Badge
                  key={status}
                  variant={STATUS_VARIANTS[status]}
                  label={t(`price_lists.import_count_${status}`, { count: preview.counts[status] })}
                />
              ))}
            </HStack>
            <Text type="supporting" color="secondary">
              {changes === 0
                ? t("price_lists.import_nothing_to_change")
                : t("price_lists.import_summary", {
                    upserts: preview.upserts,
                    clears: preview.clears,
                    changes,
                  })}
            </Text>
            {preview.rows.length > 0 && (
              <div
                style={{ maxHeight: 360, overflow: "auto", flex: "none", padding: 16, margin: -16 }}
              >
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHeaderCell>{t("price_lists.import_column_line")}</TableHeaderCell>
                      <TableHeaderCell>{t("price_lists.import_column_result")}</TableHeaderCell>
                      <TableHeaderCell>{t("price_lists.import_column_what")}</TableHeaderCell>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {preview.rows.map((row) => (
                      <TableRow key={row.line}>
                        <TableCell>{row.line}</TableCell>
                        <TableCell>
                          <Badge
                            variant={STATUS_VARIANTS[row.status]}
                            label={t(`price_lists.import_status_${row.status}`)}
                          />
                        </TableCell>
                        <TableCell>{row.reasons.join(" ")}</TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              </div>
            )}
            {preview.more > 0 && (
              <Text type="supporting" color="secondary">
                {t("price_lists.import_more_rows", { count: preview.more })}
              </Text>
            )}
          </>
        )}
        {phase.kind === "applying" && (
          <ProgressBar
            label={t("price_lists.import_applying")}
            value={phase.done}
            max={Math.max(phase.total, 1)}
            hasValueLabel
            formatValueLabel={(value, max) =>
              t("price_lists.import_progress", { done: value, total: max })
            }
          />
        )}
        {phase.kind === "applied" &&
          (phase.failedCount === 0 ? (
            <Banner
              status="success"
              title={t("price_lists.import_applied", { count: phase.total })}
            />
          ) : (
            <VStack gap={2}>
              <Banner
                status="warning"
                title={t("price_lists.import_failed_title", {
                  failed: phase.failedCount,
                  total: phase.total,
                })}
                description={t("price_lists.import_failed_description")}
              />
              <div
                style={{ maxHeight: 280, overflow: "auto", flex: "none", padding: 16, margin: -16 }}
              >
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHeaderCell>{t("price_lists.import_column_line")}</TableHeaderCell>
                      <TableHeaderCell>{t("price_lists.import_column_why")}</TableHeaderCell>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {phase.failed.slice(0, 500).map((f) => (
                      <TableRow key={f.key}>
                        <TableCell>{f.where}</TableCell>
                        <TableCell>{f.error}</TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              </div>
            </VStack>
          ))}
        <HStack gap={2} hAlign="end">
          {phase.kind === "applied" ? (
            <Button label={t("common.done")} variant="primary" onClick={close} />
          ) : (
            <>
              <Button
                label={t("common.cancel")}
                variant="ghost"
                isDisabled={phase.kind === "applying"}
                onClick={close}
              />
              <Button
                label={
                  changes > 0
                    ? t("price_lists.import_apply_changes", { count: changes })
                    : t("common.apply")
                }
                variant="primary"
                isLoading={phase.kind === "applying"}
                isDisabled={!preview || Boolean(preview.error) || changes === 0}
                onClick={() => phase.kind === "preview" && apply(phase.preview, phase.upload)}
              />
            </>
          )}
        </HStack>
      </VStack>
    </Dialog>
  );
}
