"use client";

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Text } from "@astryxdesign/core/Text";
import { TextArea } from "@astryxdesign/core/TextArea";
import { useState, useTransition } from "react";

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
  missing,
}: {
  id: string;
  /** The proposal's scopes the signed-in staff member lacks: then they can't approve it. */
  missing: string[];
}) {
  const [isRejecting, setIsRejecting] = useState(false);
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  const canDecide = missing.length === 0;

  const approve = () => startTransition(async () => setError((await approveProposal(id)).error));

  return (
    <Card>
      <VStack gap={3}>
        <VStack gap={1}>
          <Text type="display-3" as="h2">
            Decide
          </Text>
          <Text type="supporting" color="secondary">
            Approving applies exactly what's previewed below, or nothing if the store changed since.
            Rejecting tells the agent why, if you add a note.
          </Text>
        </VStack>
        {!canDecide && (
          <Banner
            status="info"
            title="You can't decide this one"
            description={`It needs ${missing.join(", ")}, which your role doesn't have.`}
          />
        )}
        {error && <Banner status="error" title={error} onDismiss={() => setError(undefined)} />}
        <HStack gap={2}>
          <Button
            label="Approve"
            variant="primary"
            isLoading={isPending && !isRejecting}
            isDisabled={!canDecide}
            onClick={approve}
          />
          <Button
            label="Reject"
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
        title="Reject this proposal?"
        subtitle="Nothing is applied. The agent can read your note when it checks the proposal."
        onOpenChange={(isOpen) => !isOpen && onClose()}
      />
      <VStack gap={4} padding={4}>
        {error && <Banner status="error" title={error} />}
        <TextArea
          label="Note for the agent"
          isOptional
          placeholder="Prices end in .99 in this store; propose again with that."
          rows={4}
          maxLength={NOTE_MAX}
          value={note}
          onChange={setNote}
        />
        <HStack gap={2} hAlign="end">
          <Button label="Cancel" variant="ghost" onClick={onClose} />
          <Button
            label="Reject"
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
