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
import { useEffect, useState } from "react";

import type { PriceList } from "@/lib/price-lists";
import { sendToStorage } from "@/lib/storage-upload";

import {
  type ImportPreviewResult,
  type ImportStepResult,
  applyPriceImport,
  discardPriceImport,
  previewPriceImport,
  startPriceImport,
} from "../actions";

const STATUS_BADGES = {
  changed: { label: "Changes", variant: "info" },
  unchanged: { label: "Same", variant: "neutral" },
  unmatched: { label: "Unmatched", variant: "warning" },
  invalid: { label: "Invalid", variant: "error" },
} as const;

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
): Promise<{ upload: string; preview: ImportPreviewResult } | { error: string } | null> {
  const started = await startPriceImport({ filename: file.name, byte_size: file.size });
  if ("error" in started) return started;
  if (!onUploaded(started.id)) return null;
  if (!(await sendToStorage(started.upload, file))) {
    return { error: "The file didn't reach storage. Check your connection and try again." };
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
  const [phase, setPhase] = useState<Phase>({ kind: "uploading" });
  const [error, setError] = useState<string>();

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
        if ("error" in result) setError(result.error);
        else setPhase({ kind: "preview", ...result });
      } catch {
        if (isCurrent) setError("The file couldn't be checked. Try again.");
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
        result = { failed: [], error: "The step couldn't be sent." };
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
        title="Import prices"
        subtitle={`Rows are matched by variant_id, else SKU, else handle and options, against every product. Nothing is written until you apply.`}
        onOpenChange={(isOpen) => !isOpen && close()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        {(phase.kind === "uploading" || phase.kind === "previewing") && !error && (
          <VStack gap={2}>
            <Text color="secondary">
              {phase.kind === "uploading"
                ? "Uploading the file…"
                : "Reading the file and matching its rows…"}
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
                  variant={STATUS_BADGES[status].variant}
                  label={`${preview.counts[status].toLocaleString("en")} ${status}`}
                />
              ))}
            </HStack>
            <Text type="supporting" color="secondary">
              {changes === 0
                ? "Nothing to change: every matched row has the prices the list has."
                : `Applying sets ${preview.upserts.toLocaleString("en")} and removes ${preview.clears.toLocaleString("en")} fixed ${changes === 1 ? "price" : "prices"}. Unmatched and invalid rows are left out.`}
            </Text>
            {preview.rows.length > 0 && (
              <div
                style={{ maxHeight: 360, overflow: "auto", flex: "none", padding: 16, margin: -16 }}
              >
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHeaderCell>Line</TableHeaderCell>
                      <TableHeaderCell>Result</TableHeaderCell>
                      <TableHeaderCell>What it does</TableHeaderCell>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {preview.rows.map((row) => (
                      <TableRow key={row.line}>
                        <TableCell>{row.line}</TableCell>
                        <TableCell>
                          <Badge
                            variant={STATUS_BADGES[row.status].variant}
                            label={STATUS_BADGES[row.status].label}
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
                And {preview.more.toLocaleString("en")} more rows like these.
              </Text>
            )}
          </>
        )}
        {phase.kind === "applying" && (
          <ProgressBar
            label="Applying"
            value={phase.done}
            max={Math.max(phase.total, 1)}
            hasValueLabel
            formatValueLabel={(value, max) =>
              `${value.toLocaleString("en")} of ${max.toLocaleString("en")}`
            }
          />
        )}
        {phase.kind === "applied" &&
          (phase.failedCount === 0 ? (
            <Banner
              status="success"
              title={`Applied: ${phase.total.toLocaleString("en")} fixed ${phase.total === 1 ? "price" : "prices"} changed.`}
            />
          ) : (
            <VStack gap={2}>
              <Banner
                status="warning"
                title={`${phase.failedCount.toLocaleString("en")} of ${phase.total.toLocaleString("en")} weren't saved`}
                description="The rest were. Fix these rows and import the file again: rows already applied show as unchanged."
              />
              <div
                style={{ maxHeight: 280, overflow: "auto", flex: "none", padding: 16, margin: -16 }}
              >
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHeaderCell>Line</TableHeaderCell>
                      <TableHeaderCell>Why</TableHeaderCell>
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
            <Button label="Done" variant="primary" onClick={close} />
          ) : (
            <>
              <Button
                label="Cancel"
                variant="ghost"
                isDisabled={phase.kind === "applying"}
                onClick={close}
              />
              <Button
                label={changes > 0 ? `Apply ${changes.toLocaleString("en")} changes` : "Apply"}
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
