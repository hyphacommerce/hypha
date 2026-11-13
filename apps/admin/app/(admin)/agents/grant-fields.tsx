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

import {
  CUSTOMER_DATA,
  GRANT_AREAS,
  PROPOSE_ONLY,
  type WriteMode,
  areaLabel,
  personalDataLabel,
} from "@/lib/agents";
import { hasScope } from "@/lib/navigation";
import { useT } from "@/lib/use-t";

import type { GrantField } from "./actions";
import type { GrantDraft, GrantProblem } from "./grant";

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
  const t = useT<"agents" | "apps">();
  const applyNote =
    cap === "propose"
      ? t("agents.grant_cap_propose", { setting: 'agents.writes = "propose"' })
      : cap === "off"
        ? t("agents.grant_cap_off", { setting: 'agents.writes = "off"' })
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
    hasScope(myScopes, scope) ? undefined : t("agents.grant_lacks_scope", { scope });
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
      ? t("agents.grant_always_proposed")
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
            label={t("agents.grant_see_area", { area: label })}
            isLabelHidden
            value={value.read.includes(read) || isReadIncluded}
            onChange={(on) => toggleRead(read, on)}
            isDisabled={isReadOnly || isReadIncluded || Boolean(lacks(read))}
            disabledMessage={
              isReadOnly
                ? undefined
                : isReadIncluded
                  ? t("agents.grant_change_includes_see")
                  : lacks(read)
            }
          />
        ) : (
          <span />
        )}
        <CheckboxInput
          label={t("agents.grant_change_area", { area: label })}
          isLabelHidden
          value={isWritten}
          onChange={(on) => toggleWrite(scope, read, on)}
          isDisabled={isReadOnly || Boolean(lacks(scope))}
          disabledMessage={isReadOnly ? undefined : lacks(scope)}
        />
        <div className="grant-mode">
          {isWritten && (
            <SegmentedControl
              label={t("agents.grant_how_area", { area: label })}
              size="sm"
              value={isProposeOnly ? "propose" : mode}
              onChange={(next) => setMode(scope, next === "apply" ? "apply" : "propose")}
              isDisabled={isReadOnly || applyBlocked !== undefined}
              disabledMessage={isReadOnly ? undefined : applyBlocked}
            >
              <SegmentedControlItem value="propose" label={t("agents.grant_mode_propose")} />
              <SegmentedControlItem value="apply" label={t("agents.grant_mode_apply")} />
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
              {t("agents.grant_store_title")}
            </Text>
            <Text type="supporting" color="secondary">
              {t("agents.grant_store_description")}
            </Text>
          </VStack>
          {applyNote && <Banner status="info" title={applyNote} />}
          {sectionProblem("read")}
          {sectionProblem("write")}
          <div>
            <div className="grant-row">
              <Text type="supporting" color="secondary">
                {t("agents.grant_column_area")}
              </Text>
              <Text type="supporting" color="secondary">
                {t("agents.grant_column_see")}
              </Text>
              <Text type="supporting" color="secondary">
                {t("agents.grant_column_change")}
              </Text>
              <Text type="supporting" color="secondary">
                {t("agents.grant_column_how")}
              </Text>
            </div>
            {GRANT_AREAS.map(({ area, canWrite }) => {
              const read = `${area}:read`;
              const label = areaLabel(t, area);
              if (canWrite) return writeRow(`${area}:write`, label, read, false);
              return (
                <div key={area} className="grant-row">
                  <VStack gap={0}>
                    <Text weight="medium">{label}</Text>
                    <Text type="supporting" color="secondary">
                      {t("agents.grant_read_only_area")}
                    </Text>
                    {problemFor(read)}
                  </VStack>
                  <CheckboxInput
                    label={t("agents.grant_see_area", { area: label })}
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
            {PROPOSE_ONLY.map(({ scope, label }) => writeRow(scope, t(label), undefined, true))}
          </div>
        </VStack>
      </Card>

      <Card>
        <VStack gap={3}>
          <VStack gap={1}>
            <Text type="display-3" as="h2">
              {t("apps.detail_personal_title")}
            </Text>
            <Text type="supporting" color="secondary">
              {t("agents.grant_personal_description")}
            </Text>
          </VStack>
          <Banner
            status="warning"
            title={t("agents.grant_personal_warning_title")}
            description={t("agents.grant_personal_warning_description")}
          />
          {sectionProblem("customer_data")}
          {CUSTOMER_DATA.map((scope) => (
            <VStack key={scope} gap={0}>
              <CheckboxInput
                label={personalDataLabel(t, scope)}
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
              {t("agents.grant_operations_title")}
            </Text>
            <Text type="supporting" color="secondary">
              {t("agents.grant_operations_description")}
            </Text>
          </VStack>
          <Switch
            label={t("agents.grant_operations_only_listed")}
            value={value.operations !== null}
            onChange={(on) => onChange({ ...value, operations: on ? "" : null })}
            isDisabled={isReadOnly}
          />
          {value.operations !== null && (
            <TextArea
              label={t("agents.grant_operations_field")}
              description={t("agents.grant_operations_field_description", {
                first: "admin.orders.list",
                second: "admin.products.update",
              })}
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
