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

import { TEMPLATES, describeProblem, templateName } from "@/lib/notifications";

import { setTemplateEnabled } from "./actions";

type Template = AdminSchemas["NotificationTemplateSummary"];

const GROUPS: { title: string; description: string; has: (t: Template) => boolean }[] = [
  {
    title: "Customer emails",
    description: "Sent to the customer about their order.",
    has: (t) => t.audience === "buyer",
  },
  {
    title: "Staff emails",
    description: "Sent to your team: new orders, invites and password resets.",
    has: (t) => t.audience === "staff",
  },
  {
    title: "Layout",
    description: "The frame every email is shown in.",
    has: (t) => !t.audience,
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
  const [error, setError] = useState<string>();
  return (
    <Card>
      <VStack gap={5}>
        <VStack gap={1}>
          <Text type="display-3" as="h2">
            Templates
          </Text>
          <Text type="supporting" color="secondary">
            Each email starts from Hypha&apos;s default. Edit one to make it yours; reverting goes
            back to the default.
          </Text>
        </VStack>
        {error && <Banner status="error" title={error} onDismiss={() => setError(undefined)} />}
        {GROUPS.map((group) => {
          const rows = templates.filter(group.has);
          if (rows.length === 0) return null;
          return (
            <VStack key={group.title} gap={2}>
              <VStack gap={0.5}>
                <Text type="label">{group.title}</Text>
                <Text type="supporting" color="secondary">
                  {group.description}
                </Text>
              </VStack>
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHeaderCell>Email</TableHeaderCell>
                    <TableHeaderCell>State</TableHeaderCell>
                    <TableHeaderCell>Sent</TableHeaderCell>
                    <TableHeaderCell>
                      <span />
                    </TableHeaderCell>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {rows.map((t) => (
                    <TemplateRow key={t.key} template={t} canWrite={canWrite} onError={setError} />
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
  const name = templateName(template.key);
  return (
    <TableRow>
      <TableCell>
        <VStack gap={0.5}>
          <Text weight="medium">{name}</Text>
          <Text type="supporting" color="secondary">
            {TEMPLATES[template.key]?.description ?? ""}
          </Text>
        </VStack>
      </TableCell>
      <TableCell>
        <HStack gap={1} wrap="wrap">
          {template.customized ? (
            <Badge label="Customized" variant="info" />
          ) : (
            <Badge label="Default" variant="neutral" />
          )}
          {!template.enabled && <Badge label="Off" variant="warning" />}
          {template.problem && (
            <Tooltip
              content={`${describeProblem(template.problem)}. Emails use the default until it's fixed.`}
            >
              <span>
                <Badge label="Broken: sending the default" variant="error" />
              </span>
            </Tooltip>
          )}
        </HStack>
      </TableCell>
      <TableCell>
        {template.can_disable ? (
          <Switch
            label={`Send ${name}`}
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
            Always
          </Text>
        )}
      </TableCell>
      <TableCell>
        <HStack hAlign="end">
          <Button
            label={canWrite ? "Edit" : "View"}
            variant="ghost"
            size="sm"
            href={`/settings/notifications/${template.key}`}
          />
        </HStack>
      </TableCell>
    </TableRow>
  );
}
