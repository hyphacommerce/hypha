"use client";

// An agent's grant as a form: per area, whether it may see it and change it, each
// change propose (a person approves it) or apply (written at once); the personal data it
// may see; and an optional allowlist of operations. Controlled: the create dialog and the
// agent's page hold the draft.

import { Banner } from "@astryxdesign/core/Banner";
import { Card } from "@astryxdesign/core/Card";
import { CheckboxInput } from "@astryxdesign/core/CheckboxInput";
import { FieldStatus } from "@astryxdesign/core/FieldStatus";
import { VStack } from "@astryxdesign/core/Layout";
import { SegmentedControl, SegmentedControlItem } from "@astryxdesign/core/SegmentedControl";
import { Switch } from "@astryxdesign/core/Switch";
import { Text } from "@astryxdesign/core/Text";
import { TextArea } from "@astryxdesign/core/TextArea";
import type { AdminSchemas } from "@hypha/sdk/admin";

import {
  CUSTOMER_DATA,
  GRANT_AREAS,
  PROPOSE_ONLY,
  type WriteMode,
  areaLabel,
  personalDataLabel,
} from "@/lib/agents";
import { hasScope } from "@/lib/navigation";

import type { Grant, GrantField } from "./actions";

/** A grant being edited; `operations` is the allowlist as typed, one per line. */
export type GrantDraft = {
  read: string[];
  write: Record<string, WriteMode>;
  customerData: string[];
  /** `null`: no allowlist, every operation the scopes allow. */
  operations: string | null;
};

export const EMPTY_GRANT: GrantDraft = { read: [], write: {}, customerData: [], operations: null };

/** A saved grant as a draft. */
export function draftOf(grant: AdminSchemas["AgentGrantResponse"]): GrantDraft {
  return {
    read: grant.read,
    write: Object.fromEntries(
      Object.entries(grant.write).map(([scope, mode]) => [
        scope,
        mode === "apply" ? "apply" : "propose",
      ]),
    ),
    customerData: grant.customer_data,
    operations: grant.operations ? grant.operations.join("\n") : null,
  };
}

/** The allowlist as the API takes it: one ID per line, blanks dropped. */
export function operationsOf(draft: GrantDraft): string[] | null {
  if (draft.operations === null) return null;
  return draft.operations
    .split(/\s+/)
    .map((op) => op.trim())
    .filter(Boolean);
}

/** The draft as the API takes a grant. */
export function grantOf(draft: GrantDraft): Grant {
  return {
    read: draft.read,
    write: draft.write,
    customer_data: draft.customerData,
    operations: operationsOf(draft),
  };
}

const sameSet = (a: readonly string[], b: readonly string[]) =>
  a.length === b.length && a.every((x) => b.includes(x));

/** Whether two drafts grant the same scopes in the same modes (allowlists aside). */
export function sameScopes(a: GrantDraft, b: GrantDraft): boolean {
  const aw = Object.entries(a.write);
  return (
    sameSet(a.read, b.read) &&
    sameSet(a.customerData, b.customerData) &&
    aw.length === Object.keys(b.write).length &&
    aw.every(([scope, mode]) => b.write[scope] === mode)
  );
}

/** Whether two drafts have the same allowlist. */
export function sameOperations(a: GrantDraft, b: GrantDraft): boolean {
  const ao = operationsOf(a);
  const bo = operationsOf(b);
  return ao === null || bo === null ? ao === bo : sameSet(ao, bo);
}

export type GrantProblem = { error: string; field?: GrantField; scope?: string };

export function GrantFields({
  value,
  onChange,
  myScopes,
  cap,
  problem,
  isReadOnly = false,
}: {
  value: GrantDraft;
  onChange: (next: GrantDraft) => void;
  /** The signed-in staff member's scopes: nobody grants what they don't hold. */
  myScopes: readonly string[];
  /** The deployment's `writes_cap`. */
  cap: string;
  /** The last save's error, shown on the field it's about. */
  problem?: GrantProblem;
  isReadOnly?: boolean;
}) {
  const applyNote =
    cap === "propose"
      ? 'This deployment lets agents only propose (agents.writes = "propose"), so apply acts as propose.'
      : cap === "off"
        ? 'This deployment lets agents only read (agents.writes = "off").'
        : undefined;

  const toggleRead = (scope: string, on: boolean) =>
    onChange({
      ...value,
      read: on ? [...new Set([...value.read, scope])] : value.read.filter((s) => s !== scope),
    });
  const toggleWrite = (scope: string, read: string | undefined, on: boolean) => {
    const write = { ...value.write };
    if (on) write[scope] = "propose";
    else delete write[scope];
    // Changing an area includes seeing it: keep the read beside the write.
    const reads = on && read ? [...new Set([...value.read, read])] : value.read;
    onChange({ ...value, write, read: reads });
  };
  const setMode = (scope: string, mode: WriteMode) =>
    onChange({ ...value, write: { ...value.write, [scope]: mode } });
  const toggleData = (scope: string, on: boolean) =>
    onChange({
      ...value,
      customerData: on
        ? [...new Set([...value.customerData, scope])]
        : value.customerData.filter((s) => s !== scope),
    });

  const lacks = (scope: string) =>
    hasScope(myScopes, scope)
      ? undefined
      : `Your role doesn't have ${scope}, so you can't give it.`;
  const problemFor = (scope: string) =>
    problem?.scope === scope ? (
      <FieldStatus type="error" message={problem.error} variant="detached" />
    ) : null;
  const sectionProblem = (field: GrantField) =>
    problem && problem.field === field && !problem.scope ? (
      <Banner status="error" title={problem.error} />
    ) : null;

  const writeRow = (
    scope: string,
    label: string,
    read: string | undefined,
    isProposeOnly: boolean,
  ) => {
    const isWritten = scope in value.write;
    const isReadIncluded = read !== undefined && isWritten;
    // Apply is for someone who could approve the same write: they hold its scope.
    const applyBlocked = isProposeOnly
      ? "Always proposed: a person approves every use."
      : (lacks(scope) ?? applyNote);
    const mode = value.write[scope] ?? "propose";
    return (
      <div key={scope} className="grant-row">
        <VStack gap={0}>
          <Text weight="medium">{label}</Text>
          {problemFor(scope)}
          {read && problemFor(read)}
        </VStack>
        {read ? (
          <CheckboxInput
            label={`See ${label.toLowerCase()}`}
            isLabelHidden
            value={value.read.includes(read) || isReadIncluded}
            onChange={(on) => toggleRead(read, on)}
            isDisabled={isReadOnly || isReadIncluded || Boolean(lacks(read))}
            disabledMessage={
              isReadOnly ? undefined : isReadIncluded ? "Changing includes seeing." : lacks(read)
            }
          />
        ) : (
          <span />
        )}
        <CheckboxInput
          label={`Change ${label.toLowerCase()}`}
          isLabelHidden
          value={isWritten}
          onChange={(on) => toggleWrite(scope, read, on)}
          isDisabled={isReadOnly || Boolean(lacks(scope))}
          disabledMessage={isReadOnly ? undefined : lacks(scope)}
        />
        <div className="grant-mode">
          {isWritten && (
            <SegmentedControl
              label={`How it changes ${label.toLowerCase()}`}
              size="sm"
              value={isProposeOnly ? "propose" : mode}
              onChange={(next) => setMode(scope, next === "apply" ? "apply" : "propose")}
              isDisabled={isReadOnly || applyBlocked !== undefined}
              disabledMessage={isReadOnly ? undefined : applyBlocked}
            >
              <SegmentedControlItem value="propose" label="Propose" />
              <SegmentedControlItem value="apply" label="Apply" />
            </SegmentedControl>
          )}
        </div>
      </div>
    );
  };

  return (
    <VStack gap={5}>
      <Card>
        <VStack gap={3}>
          <VStack gap={1}>
            <Text type="display-3" as="h2">
              Store access
            </Text>
            <Text type="supporting" color="secondary">
              What it may see and change. A change it proposes waits in the proposals inbox until
              someone approves it; one it applies is written at once. Propose is the default.
            </Text>
          </VStack>
          {applyNote && <Banner status="info" title={applyNote} />}
          {sectionProblem("read")}
          {sectionProblem("write")}
          <div>
            <div className="grant-row">
              <Text type="supporting" color="secondary">
                Area
              </Text>
              <Text type="supporting" color="secondary">
                See
              </Text>
              <Text type="supporting" color="secondary">
                Change
              </Text>
              <Text type="supporting" color="secondary">
                How
              </Text>
            </div>
            {GRANT_AREAS.map(({ area, canWrite }) => {
              const read = `${area}:read`;
              const label = areaLabel(area);
              if (canWrite) return writeRow(`${area}:write`, label, read, false);
              return (
                <div key={area} className="grant-row">
                  <VStack gap={0}>
                    <Text weight="medium">{label}</Text>
                    <Text type="supporting" color="secondary">
                      Agents may only see these.
                    </Text>
                    {problemFor(read)}
                  </VStack>
                  <CheckboxInput
                    label={`See ${label.toLowerCase()}`}
                    isLabelHidden
                    value={value.read.includes(read)}
                    onChange={(on) => toggleRead(read, on)}
                    isDisabled={isReadOnly || Boolean(lacks(read))}
                    disabledMessage={isReadOnly ? undefined : lacks(read)}
                  />
                  <span />
                  <span />
                </div>
              );
            })}
            {PROPOSE_ONLY.map((scope) =>
              writeRow(scope, "Erase customers' personal data", undefined, true),
            )}
          </div>
        </VStack>
      </Card>

      <Card>
        <VStack gap={3}>
          <VStack gap={1}>
            <Text type="display-3" as="h2">
              Customers' personal data
            </Text>
            <Text type="supporting" color="secondary">
              Off by default: without these, names, emails, phone numbers and addresses are left out
              of everything the agent reads.
            </Text>
          </VStack>
          <Banner
            status="warning"
            title="The agent's model provider sees what the agent reads"
            description="Grant personal data only when the job needs it, and only if your agreement with that provider covers it."
          />
          {sectionProblem("customer_data")}
          {CUSTOMER_DATA.map((scope) => (
            <VStack key={scope} gap={0}>
              <CheckboxInput
                label={personalDataLabel(scope)}
                description={scope}
                value={value.customerData.includes(scope)}
                onChange={(on) => toggleData(scope, on)}
                isDisabled={isReadOnly || Boolean(lacks(scope))}
                disabledMessage={isReadOnly ? undefined : lacks(scope)}
              />
              {problemFor(scope)}
            </VStack>
          ))}
        </VStack>
      </Card>

      <Card>
        <VStack gap={3}>
          <VStack gap={1}>
            <Text type="display-3" as="h2">
              Operations
            </Text>
            <Text type="supporting" color="secondary">
              Optionally, only these operations exist for the agent, within its scopes.
            </Text>
          </VStack>
          <Switch
            label="Only allow listed operations"
            value={value.operations !== null}
            onChange={(on) => onChange({ ...value, operations: on ? "" : null })}
            isDisabled={isReadOnly}
          />
          {value.operations !== null && (
            <TextArea
              label="Operations"
              description="Operation IDs as the API documents name them, one per line, like admin.orders.list or admin.products.update."
              rows={5}
              value={value.operations}
              onChange={(operations) => onChange({ ...value, operations })}
              isReadOnly={isReadOnly}
              status={
                problem?.field === "operations"
                  ? { type: "error", message: problem.error }
                  : undefined
              }
            />
          )}
        </VStack>
      </Card>
    </VStack>
  );
}
