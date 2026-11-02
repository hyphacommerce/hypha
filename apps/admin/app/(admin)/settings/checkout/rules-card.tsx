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

import { useCurrencies } from "@/components/currencies";
import {
  PRESETS,
  type Rule,
  type RuleOptions,
  appliesSummary,
  conditionSummary,
} from "@/lib/checkout-rules";

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
  const currencies = useCurrencies();
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
              Checkout rules
            </Text>
            <Text type="supporting" color="secondary">
              Evaluated on every change to a cart, in this order. At most 100.
            </Text>
          </VStack>
          {canWrite && (
            <Button label="Add rule" variant="secondary" onClick={() => setAdding(true)} />
          )}
        </HStack>
        {error && <Banner status="error" title={error} onDismiss={() => setError(undefined)} />}
        {rules.length === 0 ? (
          <EmptyState
            title="No checkout rules yet"
            description="Refuse PO boxes for couriers, require a house number or a phone, keep address lines short for labels: start from a preset."
          />
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHeaderCell>Rule</TableHeaderCell>
                <TableHeaderCell>Outcome</TableHeaderCell>
                <TableHeaderCell>Enabled</TableHeaderCell>
                <TableHeaderCell>
                  <span />
                </TableHeaderCell>
              </TableRow>
            </TableHeader>
            <TableBody>
              {rules.map((rule, index) => {
                const where = appliesSummary(rule, options);
                return (
                  <TableRow key={rule.id}>
                    <TableCell>
                      <VStack gap={0.5}>
                        <Link href={`/settings/checkout/rules/${rule.id}`} isStandalone>
                          {rule.name}
                        </Link>
                        <Text type="supporting" color="secondary">
                          {rule.conditions
                            .map((c) => conditionSummary(c, options, currencies))
                            .join(" and ")}
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
                          label={rule.outcome.severity === "error" ? "Blocks checkout" : "Warns"}
                          variant={rule.outcome.severity === "error" ? "error" : "warning"}
                        />
                        <Text type="supporting" color="secondary">
                          {rule.outcome.message}
                        </Text>
                      </VStack>
                    </TableCell>
                    <TableCell>
                      <Switch
                        label={`Enforce ${rule.name}`}
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
                            label={`Move ${rule.name} up`}
                            icon={<ArrowUpIcon />}
                            size="sm"
                            variant="ghost"
                            isDisabled={index === 0 || isMoving}
                            onClick={() => move(rule, index - 1)}
                          />
                          <IconButton
                            label={`Move ${rule.name} down`}
                            icon={<ArrowDownIcon />}
                            size="sm"
                            variant="ghost"
                            isDisabled={index === rules.length - 1 || isMoving}
                            onClick={() => move(rule, index + 1)}
                          />
                          <IconButton
                            label={`Delete ${rule.name}`}
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
  return (
    <Dialog isOpen onOpenChange={(isOpen) => !isOpen && onClose()} width={640}>
      <DialogHeader
        title="Add checkout rule"
        subtitle="Start from a preset; everything can be changed before you save."
        onOpenChange={(isOpen) => !isOpen && onClose()}
      />
      <div style={{ flex: "1 1 auto", minHeight: 0, overflowY: "auto" }}>
        <VStack gap={2} padding={4}>
          {PRESETS.map((preset) => (
            <ClickableCard
              key={preset.key}
              label={preset.title}
              href={`/settings/checkout/rules/new?preset=${preset.key}`}
              variant="muted"
              padding={3}
            >
              <VStack gap={0.5}>
                <Text weight="medium">{preset.title}</Text>
                <Text type="supporting" color="secondary">
                  {preset.description}
                </Text>
              </VStack>
            </ClickableCard>
          ))}
          <ClickableCard
            label="Blank rule"
            href="/settings/checkout/rules/new"
            variant="muted"
            padding={3}
          >
            <VStack gap={0.5}>
              <Text weight="medium">Blank rule</Text>
              <Text type="supporting" color="secondary">
                Pick the conditions yourself.
              </Text>
            </VStack>
          </ClickableCard>
        </VStack>
      </div>
    </Dialog>
  );
}

function DeleteDialog({ rule, onClose }: { rule: Rule; onClose: () => void }) {
  const [error, setError] = useState<string>();
  const [isPending, startTransition] = useTransition();
  return (
    <AlertDialog
      isOpen
      onOpenChange={(isOpen) => !isOpen && onClose()}
      title={`Delete ${rule.name}?`}
      description={error ?? "Carts stop showing its message at once."}
      actionLabel="Delete rule"
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
