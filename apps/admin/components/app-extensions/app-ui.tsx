"use client";

// Apps' blocks and actions where staff work (design note 0029, §6.1-§6.2): a block is
// the app's UI tree rendered with the admin's own components (`@hypha/app-blocks`), and
// its buttons and the app's action buttons run through the admin's server. An action
// with an input schema asks for its input first; one that names a payment point runs
// through the payment's route.

import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { CheckboxInput } from "@astryxdesign/core/CheckboxInput";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { NumberInput } from "@astryxdesign/core/NumberInput";
import { Selector } from "@astryxdesign/core/Selector";
import { Text } from "@astryxdesign/core/Text";
import { TextInput } from "@astryxdesign/core/TextInput";
import { useToast } from "@astryxdesign/core/Toast";
import type { BlockElement } from "@hypha/app-blocks";
import { HyphaBlock } from "@hypha/app-blocks/react";
import type { AdminSchemas } from "@hypha/sdk/admin";
import { useRouter } from "next/navigation";
import { useState } from "react";

import {
  type ActionContext,
  type ActionResult,
  runAppAction,
  runPaymentAction,
} from "@/app/(admin)/apps/extension-actions";

import { type FormValue, checkValues, inputOf, schemaFields } from "./schema-form";

type Action = AdminSchemas["ActionExtension"];

/** Runs `action` on `context`, then shows its message, refreshes or navigates. */
function useRunAction(context: ActionContext) {
  const router = useRouter();
  const toast = useToast();
  return async (action: Action, input?: Record<string, unknown>): Promise<ActionResult> => {
    const result = action.point
      ? context.payment_id
        ? await runPaymentAction(action.point, context.payment_id, context.order_id ?? "", input)
        : { error: `${action.label} needs a payment.` }
      : await runAppAction(action.app, action.id, context, input);
    if (result.error) {
      toast({ body: result.error, type: "error" });
      return result;
    }
    if (result.message) toast({ body: result.message });
    if (result.navigate) router.push(result.navigate);
    else if (result.refresh) router.refresh();
    return result;
  };
}

function isTree(value: unknown): value is { version: number; root: BlockElement } {
  return (
    typeof value === "object" &&
    value !== null &&
    typeof Reflect.get(value, "version") === "number" &&
    typeof Reflect.get(value, "root") === "object"
  );
}

/**
 * One app's block: its tree, or a quiet note when the app didn't answer. `actions` are
 * the app's actions for the target, which the block's buttons name by ID.
 */
export function AppBlockCard({
  appKey,
  appName,
  tree,
  actions,
  context,
  pagePath,
}: {
  appKey: string;
  appName: string;
  tree: unknown;
  actions: Action[];
  context: ActionContext;
  /** The admin path of the app's first page, for `navigate` buttons; none without one. */
  pagePath: string | null;
}) {
  const router = useRouter();
  const toast = useToast();
  const run = useRunAction(context);
  if (!isTree(tree)) {
    return (
      <Card>
        <Text type="supporting" color="secondary">
          {appName} didn&apos;t answer.
        </Text>
      </Card>
    );
  }
  return (
    <HyphaBlock
      tree={tree}
      onAction={async (id, values) => {
        const action = actions.find((a) => a.id === id);
        // A button for an action the app doesn't register for this target, or a form:
        // the actions route checks it either way.
        await run(
          action ?? { app: appKey, id, label: id, target: "", input_schema: null, point: null },
          values,
        );
      }}
      onNavigate={(path) => {
        if (!pagePath) {
          toast({ body: `${appName} has no page to open.`, type: "error" });
          return;
        }
        const query = path === "/" ? "" : `?path=${encodeURIComponent(path)}`;
        router.push(`${pagePath}${query}`);
      }}
    />
  );
}

/** The apps' action buttons for one target, each with its form when it takes input. */
export function AppActionButtons({
  actions,
  context,
  appNames,
}: {
  actions: Action[];
  context: ActionContext;
  appNames: Record<string, string>;
}) {
  const run = useRunAction(context);
  const [busy, setBusy] = useState<string>();
  const [asking, setAsking] = useState<Action>();
  if (actions.length === 0) return null;
  return (
    <>
      <HStack gap={2} wrap="wrap">
        {actions.map((action) => {
          const id = `${action.app}/${action.id}`;
          return (
            <Button
              key={id}
              label={action.label}
              size="sm"
              variant="secondary"
              tooltip={appNames[action.app] ?? action.app}
              isLoading={busy === id}
              onClick={async () => {
                if (schemaFields(action.input_schema).length > 0) {
                  setAsking(action);
                  return;
                }
                setBusy(id);
                try {
                  await run(action);
                } finally {
                  setBusy(undefined);
                }
              }}
            />
          );
        })}
      </HStack>
      {asking && (
        <ActionForm
          action={asking}
          appName={appNames[asking.app] ?? asking.app}
          onSubmit={(input) => run(asking, input)}
          onClose={() => setAsking(undefined)}
        />
      )}
    </>
  );
}

/** An action's input, from its JSON Schema (flat: text, numbers, checkboxes, choices). */
function ActionForm({
  action,
  appName,
  onSubmit,
  onClose,
}: {
  action: Action;
  appName: string;
  onSubmit: (input: Record<string, unknown>) => Promise<ActionResult>;
  onClose: () => void;
}) {
  const fields = schemaFields(action.input_schema);
  const [values, setValues] = useState<Record<string, FormValue>>({});
  const [problems, setProblems] = useState<Record<string, string>>({});
  const [error, setError] = useState<string>();
  const [pending, setPending] = useState(false);
  const set = (name: string, value: FormValue) => setValues((v) => ({ ...v, [name]: value }));

  const submit = async () => {
    const found = checkValues(fields, values);
    setProblems(found);
    if (Object.keys(found).length > 0) return;
    setPending(true);
    let result: ActionResult;
    try {
      result = await onSubmit(inputOf(fields, values));
    } finally {
      setPending(false);
    }
    if (result.error) setError(result.error);
    else onClose();
  };

  return (
    <Dialog isOpen onOpenChange={(isOpen) => !isOpen && onClose()} width={480} purpose="form">
      <DialogHeader
        title={action.label}
        subtitle={`Sent to ${appName}.`}
        onOpenChange={(isOpen) => !isOpen && onClose()}
      />
      <VStack gap={3} padding={4}>
        {error && <Banner status="error" title={error} />}
        {fields.map((field) => {
          const status = problems[field.name]
            ? { type: "error" as const, message: problems[field.name] }
            : undefined;
          const value = values[field.name];
          switch (field.kind) {
            case "text":
              return (
                <TextInput
                  key={field.name}
                  label={field.label}
                  description={field.description}
                  isRequired={field.required}
                  status={status}
                  value={typeof value === "string" ? value : ""}
                  onChange={(next) => set(field.name, next)}
                />
              );
            case "number":
              return (
                <NumberInput
                  key={field.name}
                  label={field.label}
                  description={field.description}
                  isRequired={field.required}
                  status={status}
                  min={field.min}
                  max={field.max}
                  value={typeof value === "number" ? value : null}
                  onChange={(next: number | null) => set(field.name, next)}
                />
              );
            case "enum":
              return (
                <Selector
                  key={field.name}
                  label={field.label}
                  description={field.description}
                  isRequired={field.required}
                  status={status}
                  options={field.options.map((o) => ({ value: o, label: o }))}
                  value={typeof value === "string" ? value : undefined}
                  onChange={(next: string) => set(field.name, next)}
                />
              );
            case "boolean":
              return (
                <CheckboxInput
                  key={field.name}
                  label={field.label}
                  description={field.description}
                  value={value === true}
                  onChange={(checked) => set(field.name, checked)}
                />
              );
          }
        })}
        <HStack gap={2} hAlign="end">
          <Button label="Cancel" variant="secondary" onClick={onClose} />
          <Button label={action.label} variant="primary" isLoading={pending} onClick={submit} />
        </HStack>
      </VStack>
    </Dialog>
  );
}
