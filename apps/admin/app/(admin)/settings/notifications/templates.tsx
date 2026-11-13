"use client";

// The store's email templates: the layout, the customer's and the staff's, each with its
// state, a switch where it can be off, and a link to its editor.

import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { HStack, VStack } from "@astryxdesign/core/Layout";
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
import { Tooltip } from "@astryxdesign/core/Tooltip";
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { useState } from "react";

import type { KeyIn } from "@/lib/messages";
import {
  describeProblem,
  templateDescription,
  templateHref,
  templateName,
} from "@/lib/notifications";
import { useFormat } from "@/lib/use-format";
import { useT } from "@/lib/use-t";

import { setTemplateEnabled } from "./actions";

type Template = AdminSchemas["NotificationTemplateListItem"];

type GroupKey = KeyIn<"notifications">;

const GROUPS: { title: GroupKey; description: GroupKey; has: (row: Template) => boolean }[] = [
  {
    title: "notifications.templates_group_buyer",
    description: "notifications.templates_group_buyer_description",
    has: (row) => row.audience === "buyer",
  },
  {
    title: "notifications.templates_group_staff",
    description: "notifications.templates_group_staff_description",
    has: (row) => row.audience === "staff",
  },
  {
    title: "notifications.templates_group_layout",
    description: "notifications.templates_group_layout_description",
    has: (row) => !row.audience,
  },
];

export function TemplatesCard({
  templates,
  canWrite,
}: {
  /** As the API lists them: the layout, the customer's, then the staff's. */
  templates: Template[];
  canWrite: boolean;
}) {
  const t = useT<"notifications">();
  const [error, setError] = useState<string>();
  return (
    <Card>
      <VStack gap={5}>
        <VStack gap={1}>
          <Text type="display-3" as="h2">
            {t("notifications.templates_title")}
          </Text>
          <Text type="supporting" color="secondary">
            {t("notifications.templates_description")}
          </Text>
        </VStack>
        {error && <Banner status="error" title={error} onDismiss={() => setError(undefined)} />}
        {GROUPS.map((group) => {
          const rows = templates.filter(group.has);
          if (rows.length === 0) return null;
          return (
            <VStack key={group.title} gap={2}>
              <VStack gap={0.5}>
                <Text type="label">{t(group.title)}</Text>
                <Text type="supporting" color="secondary">
                  {t(group.description)}
                </Text>
              </VStack>
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHeaderCell>{t("notifications.templates_column_email")}</TableHeaderCell>
                    <TableHeaderCell>{t("notifications.templates_column_state")}</TableHeaderCell>
                    <TableHeaderCell>{t("notifications.templates_column_sent")}</TableHeaderCell>
                    <TableHeaderCell>
                      <span />
                    </TableHeaderCell>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {rows.map((row) => (
                    <TemplateRow
                      key={row.key}
                      template={row}
                      canWrite={canWrite}
                      onError={setError}
                    />
                  ))}
                </TableBody>
              </Table>
            </VStack>
          );
        })}
      </VStack>
    </Card>
  );
}

function TemplateRow({
  template,
  canWrite,
  onError,
}: {
  template: Template;
  canWrite: boolean;
  onError: (error: string | undefined) => void;
}) {
  const t = useT<"notifications">();
  const format = useFormat();
  const name = templateName(t, template.key);
  return (
    <TableRow>
      <TableCell>
        <VStack gap={0.5}>
          <Text weight="medium">{name}</Text>
          <Text type="supporting" color="secondary">
            {templateDescription(t, template.key) ?? ""}
          </Text>
        </VStack>
      </TableCell>
      <TableCell>
        <HStack gap={1} wrap="wrap">
          {template.customized ? (
            <Badge label={t("notifications.template_customized")} variant="info" />
          ) : (
            <Badge label={t("common.default")} variant="neutral" />
          )}
          {template.languages.map((tag) => (
            <Badge
              key={tag}
              label={t("notifications.template_customized_in", {
                language: format.languageName(tag),
              })}
              variant="info"
            />
          ))}
          {!template.enabled && <Badge label={t("common.off")} variant="warning" />}
          {template.problem && (
            <Tooltip
              content={t("notifications.template_broken_tooltip", {
                problem: describeProblem(t, template.problem),
              })}
            >
              <span>
                <Badge label={t("notifications.template_broken")} variant="error" />
              </span>
            </Tooltip>
          )}
        </HStack>
      </TableCell>
      <TableCell>
        {template.can_disable ? (
          <Switch
            label={t("notifications.template_send_label", { name })}
            isLabelHidden
            size="sm"
            value={template.enabled}
            isDisabled={!canWrite}
            changeAction={async (enabled) =>
              onError((await setTemplateEnabled(template.key, enabled)).error)
            }
          />
        ) : (
          <Text type="supporting" color="secondary">
            {t("notifications.template_always")}
          </Text>
        )}
      </TableCell>
      <TableCell>
        <HStack hAlign="end">
          <Button
            label={canWrite ? t("common.edit") : t("common.view")}
            variant="ghost"
            size="sm"
            href={templateHref(template.key, null)}
          />
        </HStack>
      </TableCell>
    </TableRow>
  );
}
