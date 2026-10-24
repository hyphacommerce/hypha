"use client";

// Importing a CSV file into a list: the server matches its rows and says what they'd
// do, nothing written; applying sends the changes in steps of up to 1,000, with the
// progress shown, and lists any row the API refused.

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

import { chunk } from "@/lib/price-csv";
import type { PriceGridDiff } from "@/lib/price-grid/types";
import type { PriceList } from "@/lib/price-lists";

import { type ImportPreviewResult, applyImportChunk, previewImport } from "../actions";

const STATUS_BADGES = {
  changed: { label: "Changes", variant: "info" },
  unchanged: { label: "Same", variant: "neutral" },
  unmatched: { label: "Unmatched", variant: "warning" },
  invalid: { label: "Invalid", variant: "error" },
} as const;

/** The steps applying takes: each at most 1,000 prices to set, or 1,000 to remove. */
function stepsOf(diff: PriceGridDiff): PriceGridDiff[] {
  return [
    ...chunk(diff.upserts).map((upserts) => ({ upserts, clears: [] })),
    ...chunk(diff.clears).map((clears) => ({ upserts: [], clears })),
  ];
}

type Phase =
  | { kind: "previewing" }
  | { kind: "preview"; preview: ImportPreviewResult }
  | { kind: "applying"; done: number; total: number }
  | { kind: "applied"; failed: { line?: number; variant: string; error: string }[]; total: number };

export function ImportDialog({
  list,
  text,
  onClose,
}: {
  list: PriceList;
  /** The file's contents. */
  text: string;
  /** `isApplied`: something was written, so the page's prices are stale. */
  onClose: (isApplied: boolean) => void;
}) {
  const [phase, setPhase] = useState<Phase>({ kind: "previewing" });
  const [error, setError] = useState<string>();

  useEffect(() => {
    let isCurrent = true;
    previewImport(list.id, text).then(
      (preview) => isCurrent && setPhase({ kind: "preview", preview }),
      () => isCurrent && setError("The file couldn't be checked. Try again."),
    );
    return () => {
      isCurrent = false;
    };
  }, [list.id, text]);

  const apply = async (preview: ImportPreviewResult) => {
    const steps = stepsOf(preview.diff);
    const total = preview.diff.upserts.length + preview.diff.clears.length;
    const lineOf = new Map(
      preview.rows.flatMap((r) => (r.variant ? [[r.variant, r.line] as const] : [])),
    );
    const failed: { line?: number; variant: string; error: string }[] = [];
    let done = 0;
    setPhase({ kind: "applying", done, total });
    // One step at a time, on purpose: the progress bar moves with each.
    for (const step of steps) {
      let result;
      try {
        result = await applyImportChunk(list.id, step);
      } catch {
        result = { error: "The step couldn't be sent." };
      }
      const variants = [
        ...step.upserts.map((u) => u.variant),
        ...step.clears.map((c) => c.variant),
      ];
      if (result.rowErrors) {
        for (const [variant, message] of Object.entries(result.rowErrors)) {
          failed.push({ variant, line: lineOf.get(variant), error: message });
        }
      } else if (result.error) {
        for (const variant of variants) {
          failed.push({ variant, line: lineOf.get(variant), error: result.error });
        }
      }
      done += variants.length;
      setPhase({ kind: "applying", done, total });
    }
    setPhase({ kind: "applied", failed, total });
  };

  const close = () => onClose(phase.kind === "applied" || phase.kind === "applying");
  const preview = phase.kind === "preview" ? phase.preview : undefined;
  const changes = preview ? preview.diff.upserts.length + preview.diff.clears.length : 0;

  return (
    <Dialog isOpen onOpenChange={(isOpen) => !isOpen && close()} width={820} purpose="form">
      <DialogHeader
        title="Import prices"
        subtitle={`Rows are matched by variant_id, else SKU, else handle and options, against every product. Nothing is written until you apply.`}
        onOpenChange={(isOpen) => !isOpen && close()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        {phase.kind === "previewing" && !error && (
          <VStack gap={2}>
            <Text color="secondary">Reading the file and matching its rows…</Text>
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
                : `Applying sets ${preview.diff.upserts.length.toLocaleString("en")} and removes ${preview.diff.clears.length.toLocaleString("en")} fixed ${changes === 1 ? "price" : "prices"}. Unmatched and invalid rows are left out.`}
            </Text>
            {preview.rows.length > 0 && (
              <div style={{ maxHeight: 360, overflow: "auto" }}>
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
          (phase.failed.length === 0 ? (
            <Banner
              status="success"
              title={`Applied: ${phase.total.toLocaleString("en")} fixed ${phase.total === 1 ? "price" : "prices"} changed.`}
            />
          ) : (
            <VStack gap={2}>
              <Banner
                status="warning"
                title={`${phase.failed.length.toLocaleString("en")} of ${phase.total.toLocaleString("en")} weren't saved`}
                description="The rest were. Fix these rows and import the file again: rows already applied show as unchanged."
              />
              <div style={{ maxHeight: 280, overflow: "auto" }}>
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHeaderCell>Line</TableHeaderCell>
                      <TableHeaderCell>Why</TableHeaderCell>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {phase.failed.slice(0, 500).map((f) => (
                      <TableRow key={f.variant}>
                        <TableCell>{f.line ?? f.variant}</TableCell>
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
                onClick={() => preview && apply(preview)}
              />
            </>
          )}
        </HStack>
      </VStack>
    </Dialog>
  );
}
