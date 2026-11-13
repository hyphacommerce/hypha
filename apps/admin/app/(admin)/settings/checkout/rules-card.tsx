"use client";

// Checkout rules: the list in the order they're evaluated, adding one from a preset,
// switching on and off, moving up and down, and deleting. Editing is its own page.

import { AlertDialog } from "@astryxdesign/core/AlertDialog";
import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { ClickableCard } from "@astryxdesign/core/ClickableCard";
import { Dialog, DialogHeader } from "@astryxdesign/core/Dialog";
import { EmptyState } from "@astryxdesign/core/EmptyState";
import { IconButton } from "@astryxdesign/core/IconButton";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { Switch } from "@astryxdesign/core/Switch";
import {
  Table,
  TableBody,
  TableCell,
  TableHeader,
  TableHeaderCell,
  TableRow,
} from "@astryxdesign/core/Table";
import { Text } from "@astryxdesign/core/Text";
import { ArrowDownIcon, ArrowUpIcon, TrashIcon } from "@heroicons/react/24/outline";
import { useState, useTransition } from "react";

import {
  PRESETS,
  type Rule,
  type RuleOptions,
  appliesSummary,
  conditionSummary,
} from "@/lib/checkout-rules";
import { useFormat } from "@/lib/use-format";
import { useT } from "@/lib/use-t";

import { deleteRule, moveRule, setRuleEnabled } from "./actions";

export function RulesCard({
  rules,
  options,
  canWrite,
}: {
  /** By position, then ID: as the API lists them. */
  rules: Rule[];
  options: RuleOptions;
  canWrite: boolean;
}) {
  const t = useT<"checkout">();
  const format = useFormat();
  const [isAdding, setAdding] = useState(false);
  const [deleting, setDeleting] = useState<Rule>();
  const [error, setError] = useState<string>();
  const [isMoving, startMoving] = useTransition();

  const move = (rule: Rule, to: number) =>
    startMoving(async () => setError((await moveRule(rule.id, to)).error));

  return (
    <Card>
      <VStack gap={3}>
        <HStack gap={2} hAlign="between" vAlign="center" wrap="wrap">
          <VStack gap={1}>
            <Text type="display-3" as="h2">
              {t("checkout.rules_title")}
            </Text>
            <Text type="supporting" color="secondary">
              {t("checkout.rules_description")}
            </Text>
          </VStack>
          {canWrite && (
            <Button
              label={t("checkout.rules_add")}
              variant="secondary"
              onClick={() => setAdding(true)}
            />
          )}
        </HStack>
        {error && <Banner status="error" title={error} onDismiss={() => setError(undefined)} />}
        {rules.length === 0 ? (
          <EmptyState
            title={t("checkout.rules_empty_title")}
            description={t("checkout.rules_empty_description")}
          />
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHeaderCell>{t("checkout.rules_column_rule")}</TableHeaderCell>
                <TableHeaderCell>{t("checkout.rules_column_outcome")}</TableHeaderCell>
                <TableHeaderCell>{t("common.enabled")}</TableHeaderCell>
                <TableHeaderCell>
                  <span />
                </TableHeaderCell>
              </TableRow>
            </TableHeader>
            <TableBody>
              {rules.map((rule, index) => {
                const where = appliesSummary(t, format, rule, options);
                return (
                  <TableRow key={rule.id}>
                    <TableCell>
                      <VStack gap={0.5}>
                        <Link href={`/settings/checkout/rules/${rule.id}`} isStandalone>
                          {rule.name}
                        </Link>
                        <Text type="supporting" color="secondary">
                          {format.list(
                            rule.conditions.map((c) => conditionSummary(t, format, c, options)),
                            "conjunction",
                          )}
                        </Text>
                        {where && (
                          <Text type="supporting" color="secondary">
                            {where}
                          </Text>
                        )}
                      </VStack>
                    </TableCell>
                    <TableCell>
                      <VStack gap={0.5}>
                        <Badge
                          label={
                            rule.outcome.severity === "error"
                              ? t("checkout.outcome_blocks")
                              : t("checkout.outcome_warns")
                          }
                          variant={rule.outcome.severity === "error" ? "error" : "warning"}
                        />
                        <Text type="supporting" color="secondary">
                          {rule.outcome.message}
                        </Text>
                      </VStack>
                    </TableCell>
                    <TableCell>
                      <Switch
                        label={t("checkout.rule_enforce", { name: rule.name })}
                        isLabelHidden
                        size="sm"
                        value={rule.enabled}
                        isDisabled={!canWrite}
                        changeAction={async (enabled) =>
                          setError((await setRuleEnabled(rule.id, enabled)).error)
                        }
                      />
                    </TableCell>
                    <TableCell>
                      {canWrite && (
                        <HStack gap={1} hAlign="end">
                          <IconButton
                            label={t("checkout.rule_move_up", { name: rule.name })}
                            icon={<ArrowUpIcon />}
                            size="sm"
                            variant="ghost"
                            isDisabled={index === 0 || isMoving}
                            onClick={() => move(rule, index - 1)}
                          />
                          <IconButton
                            label={t("checkout.rule_move_down", { name: rule.name })}
                            icon={<ArrowDownIcon />}
                            size="sm"
                            variant="ghost"
                            isDisabled={index === rules.length - 1 || isMoving}
                            onClick={() => move(rule, index + 1)}
                          />
                          <IconButton
                            label={t("checkout.rule_delete", { name: rule.name })}
                            icon={<TrashIcon />}
                            size="sm"
                            variant="ghost"
                            onClick={() => setDeleting(rule)}
                          />
                        </HStack>
                      )}
                    </TableCell>
                  </TableRow>
                );
              })}
            </TableBody>
          </Table>
        )}
      </VStack>
      {isAdding && <PresetDialog onClose={() => setAdding(false)} />}
      {deleting && <DeleteDialog rule={deleting} onClose={() => setDeleting(undefined)} />}
    </Card>
  );
}

/** "Add rule": a preset to start from, or a blank rule. */
function PresetDialog({ onClose }: { onClose: () => void }) {
  const t = useT<"checkout">();
  return (
    <Dialog isOpen onOpenChange={(isOpen) => !isOpen && onClose()} width={640}>
      <DialogHeader
        title={t("checkout.preset_dialog_title")}
        subtitle={t("checkout.preset_dialog_subtitle")}
        onOpenChange={(isOpen) => !isOpen && onClose()}
      />
      <div style={{ flex: "1 1 auto", minHeight: 0, overflowY: "auto" }}>
        <VStack gap={2} padding={4}>
          {PRESETS.map((preset) => (
            <ClickableCard
              key={preset.key}
              label={t(`checkout.preset_${preset.key}_title`)}
              href={`/settings/checkout/rules/new?preset=${preset.key}`}
              variant="muted"
              padding={3}
            >
              <VStack gap={0.5}>
                <Text weight="medium">{t(`checkout.preset_${preset.key}_title`)}</Text>
                <Text type="supporting" color="secondary">
                  {t(`checkout.preset_${preset.key}_description`)}
                </Text>
              </VStack>
            </ClickableCard>
          ))}
          <ClickableCard
            label={t("checkout.preset_blank_title")}
            href="/settings/checkout/rules/new"
            variant="muted"
            padding={3}
          >
            <VStack gap={0.5}>
              <Text weight="medium">{t("checkout.preset_blank_title")}</Text>
              <Text type="supporting" color="secondary">
                {t("checkout.preset_blank_description")}
              </Text>
            </VStack>
          </ClickableCard>
        </VStack>
      </div>
    </Dialog>
  );
}

function DeleteDialog({ rule, onClose }: { rule: Rule; onClose: () => void }) {
  const t = useT<"checkout">();
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  return (
    <AlertDialog
      isOpen
      onOpenChange={(isOpen) => !isOpen && onClose()}
      title={t("common.delete_named", { name: rule.name })}
      description={error ?? t("checkout.rule_delete_description")}
      actionLabel={t("checkout.rule_delete_action")}
      isActionLoading={isPending}
      onAction={() =>
        startTransition(async () => {
          const result = await deleteRule(rule.id);
          if (result.error) setError(result.error);
          else onClose();
        })
      }
    />
  );
}
