"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Text } from "@astryxdesign/core/Text";
import { TextArea } from "@astryxdesign/core/TextArea";
import { useState, useTransition } from "react";

import { useFormat } from "@/lib/use-format";
import { useT } from "@/lib/use-t";

import { approveProposal, rejectProposal } from "../../actions";

/** The longest note the API keeps. */
const NOTE_MAX = 2000;

/**
 * Approving or rejecting a pending proposal. Either way the page reads the proposal
 * again and shows how it was decided; only an approval or rejection that didn't get that
 * far shows its error here.
 */
export function Decision({
  id,
  refreshes,
  missing,
}: {
  id: string;
  /** How many times it was previewed again: what's shown, which approving names. */
  refreshes: number;
  /** The proposal's scopes the signed-in staff member lacks: then they can't approve it. */
  missing: string[];
}) {
  const t = useT<"agents">();
  const format = useFormat();
  const [isRejecting, setIsRejecting] = useState(false);
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const canDecide = missing.length === 0;

  const approve = () =>
    startTransition(async () => setError((await approveProposal(id, refreshes)).error));

  return (
    <Card>
      <VStack gap={3}>
        <VStack gap={1}>
          <Text type="display-3" as="h2">
            {t("agents.decide_title")}
          </Text>
          <Text type="supporting" color="secondary">
            {t("agents.decide_description")}
          </Text>
        </VStack>
        {!canDecide && (
          <Banner
            status="info"
            title={t("agents.decide_cannot_title")}
            description={t("agents.decide_cannot_description", {
              scopes: format.list(missing, "unit"),
            })}
          />
        )}
        {error && <Banner status="error" title={error} onDismiss={() => setError(undefined)} />}
        <HStack gap={2}>
          <Button
            label={t("common.approve")}
            variant="primary"
            isLoading={isPending && !isRejecting}
            isDisabled={!canDecide}
            onClick={approve}
          />
          <Button
            label={t("agents.decide_reject")}
            variant="secondary"
            isDisabled={!canDecide || isPending}
            onClick={() => setIsRejecting(true)}
          />
        </HStack>
      </VStack>
      {isRejecting && <RejectDialog id={id} onClose={() => setIsRejecting(false)} />}
    </Card>
  );
}

function RejectDialog({ id, onClose }: { id: string; onClose: () => void }) {
  const t = useT<"agents">();
  const [note, setNote] = useState("");
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const isTooLong = note.length > NOTE_MAX;

  const reject = () =>
    startTransition(async () => {
      const result = await rejectProposal(id, note);
      if (result.error) setError(result.error);
      else onClose();
    });

  return (
    <Dialog isOpen onOpenChange={(isOpen) => !isOpen && onClose()} width={520} purpose="form">
      <DialogHeader
        title={t("agents.reject_dialog_title")}
        subtitle={t("agents.reject_dialog_subtitle")}
        onOpenChange={(isOpen) => !isOpen && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <TextArea
          label={t("agents.reject_note_label")}
          isOptional
          placeholder={t("agents.reject_note_placeholder")}
          rows={4}
          maxLength={NOTE_MAX}
          value={note}
          onChange={setNote}
        />
        <HStack gap={2} hAlign="end">
          <Button label={t("common.cancel")} variant="ghost" onClick={onClose} />
          <Button
            label={t("agents.decide_reject")}
            variant="destructive"
            isLoading={isPending}
            isDisabled={isTooLong}
            onClick={reject}
          />
        </HStack>
      </VStack>
    </Dialog>
  );
}
