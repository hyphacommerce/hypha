import { Badge } from "@astryxdesign/core/Badge";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { MetadataList, MetadataListItem } from "@astryxdesign/core/MetadataList";
import { Section } from "@astryxdesign/core/Section";
import {
  Table,
  TableBody,
  TableCell,
  TableHeader,
  TableHeaderCell,
  TableRow,
} from "@astryxdesign/core/Table";
import { Text } from "@astryxdesign/core/Text";
import type { ReactNode } from "react";

import { DateTime } from "@/components/date-time";
import { PageHeader } from "@/components/page-header";
import { actorLabel, appState, isPersonalDataScope, personalDataLabel } from "@/lib/apps";
import { loaded, requireStaff, sessionClient } from "@/lib/hypha";

import { AppActions } from "./app-actions";
import { ReduceAccess } from "./reduce-access";

const HISTORY_STATUS: Record<string, { label: string; variant: "success" | "warning" | "error" }> =
  {
    pending: { label: "Waiting", variant: "warning" },
    approved: { label: "Approved", variant: "success" },
    denied: { label: "Denied", variant: "error" },
  };

function Panel({
  title,
  description,
  action,
  children,
}: {
  title: string;
  description?: string;
  action?: ReactNode;
  children: ReactNode;
}) {
  return (
    <Card>
      <VStack gap={3}>
        <HStack gap={2} hAlign="between" vAlign="start" wrap="wrap">
          <VStack gap={1}>
            <Text type="display-3" as="h2">
              {title}
            </Text>
            {description && (
              <Text type="supporting" color="secondary">
                {description}
              </Text>
            )}
          </VStack>
          {action}
        </HStack>
        {children}
      </VStack>
    </Card>
  );
}

/**
 * One app (design note 0029): what it may do, which personal data it sees, where its
 * events go, and how it was authorized. The apps platform's own pages for the app
 * (`apps.page`, `settings.app`, §5) will hang off this page.
 */
export default async function AppPage({ params }: { params: Promise<{ key: string }> }) {
  const [{ key }, client, me] = await Promise.all([params, sessionClient(), requireStaff()]);
  const app = loaded(await client.GET("/admin/apps/{key}", { params: { path: { key } } }));
  const state = appState(app);
  const scopes = app.grant?.scopes ?? [];
  const personal = scopes.filter(isPersonalDataScope);
  const other = scopes.filter((scope) => !isPersonalDataScope(scope));
  const canWrite = me.scopes.includes("apps:write");

  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={960}>
        <PageHeader
          title={app.name}
          back={{ label: "Apps", href: "/apps" }}
          badges={<Badge label={state.label} variant={state.variant} />}
          description={`${app.key} · ${state.description}`}
          actions={
            canWrite && (
              <AppActions
                appKey={app.key}
                name={app.name}
                isEnabled={app.enabled}
                isInstalled={app.installed}
                isUninstalled={Boolean(app.uninstalled_at)}
              />
            )
          }
        />

        <Panel
          title="Access"
          description={
            app.grant
              ? "What the app's tokens may do, at most: approved by staff when it was installed."
              : "This app has no OAuth grant: it can't call the API, only receive webhooks."
          }
          action={
            canWrite &&
            app.grant &&
            app.installed &&
            !app.uninstalled_at &&
            scopes.length > 0 && <ReduceAccess appKey={app.key} name={app.name} scopes={scopes} />
          }
        >
          {app.grant ? (
            <VStack gap={3}>
              <HStack gap={1} wrap="wrap">
                {other.length === 0 ? (
                  <Text color="secondary">No store scopes.</Text>
                ) : (
                  other.map((scope) => <Badge key={scope} label={scope} />)
                )}
              </HStack>
              <Text type="supporting" color="secondary">
                Approved by {actorLabel(app.grant.approved_by)},{" "}
                <DateTime value={app.grant.approved_at} short />.
              </Text>
            </VStack>
          ) : (
            <Text color="secondary">
              Custom apps are set up under Settings → Developers → Webhooks and custom apps.
            </Text>
          )}
        </Panel>

        <Panel
          title="Customers' personal data"
          description="Without these scopes Hypha leaves names, emails, phone numbers and addresses out of everything the app reads or receives."
        >
          {personal.length === 0 ? (
            <Text color="secondary">This app sees no personal data.</Text>
          ) : (
            <VStack gap={2}>
              {personal.map((scope) => (
                <HStack key={scope} gap={2} vAlign="center">
                  <Badge label={scope} variant="warning" />
                  <Text>{personalDataLabel(scope)}</Text>
                </HStack>
              ))}
            </VStack>
          )}
        </Panel>

        <Panel
          title="Webhooks"
          description={
            app.webhook ? `Events go to ${app.webhook.url}.` : "This app takes no webhooks."
          }
          action={
            app.webhook && (
              <Button
                label="See deliveries"
                variant="secondary"
                href={`/apps/${app.key}/deliveries`}
              />
            )
          }
        >
          {app.webhook &&
            (app.subscriptions.length === 0 ? (
              <Badge label="No events" variant="warning" />
            ) : (
              <HStack gap={1} wrap="wrap">
                {app.subscriptions.map((pattern) => (
                  <Badge key={pattern} label={pattern} />
                ))}
              </HStack>
            ))}
        </Panel>

        {app.manifest && (
          <MetadataList title="Installed from">
            <MetadataListItem label="Manifest">
              <Text type="code">{app.manifest.url}</Text>
            </MetadataListItem>
            <MetadataListItem label="Read">
              <DateTime value={app.manifest.fetched_at} short />
            </MetadataListItem>
            <MetadataListItem label="Added">
              <DateTime value={app.created_at} short />
            </MetadataListItem>
          </MetadataList>
        )}

        {app.history.length > 0 && (
          <Panel title="Authorization history" description="Each time the app asked for access.">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHeaderCell>Asked</TableHeaderCell>
                  <TableHeaderCell>Scopes</TableHeaderCell>
                  <TableHeaderCell>Answer</TableHeaderCell>
                </TableRow>
              </TableHeader>
              <TableBody>
                {app.history.map((entry) => {
                  const status = HISTORY_STATUS[entry.status] ?? {
                    label: entry.status,
                    variant: "warning" as const,
                  };
                  return (
                    <TableRow key={entry.id}>
                      <TableCell>
                        <DateTime value={entry.created_at} short />
                      </TableCell>
                      <TableCell>
                        <Text type="supporting">{entry.scopes.join(", ") || "None"}</Text>
                      </TableCell>
                      <TableCell>
                        <VStack gap={0}>
                          <Badge label={status.label} variant={status.variant} />
                          {entry.decided_at && (
                            <Text type="supporting" color="secondary">
                              by {actorLabel(entry.decided_by)}
                            </Text>
                          )}
                        </VStack>
                      </TableCell>
                    </TableRow>
                  );
                })}
              </TableBody>
            </Table>
          </Panel>
        )}
      </VStack>
    </Section>
  );
}
