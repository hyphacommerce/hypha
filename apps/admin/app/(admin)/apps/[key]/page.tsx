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
import {
  actorLabel,
  appState,
  isBuiltIn,
  isPersonalDataScope,
  personalDataLabel,
} from "@/lib/apps";
import { loadExtensions } from "@/lib/app-extensions";
import { loaded, requireStaff, sessionClient } from "@/lib/hypha";

import { AppActions } from "./app-actions";
import { AppPageTabs } from "./app-pages";
import { ReduceAccess } from "./reduce-access";
import { SporeSettingsForm } from "./spore-settings";
import { SporeSwitch } from "./spore-switch";

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
 * events go, and how it was authorized. A spore built into the API (design note 0030)
 * shows its switch and its settings form instead of uninstalling and webhooks. The app's own pages (`apps.page`,
 * `settings.app`, §5) are linked above, each at `/apps/{key}/pages/{id}`.
 */
export default async function AppPage({ params }: { params: Promise<{ key: string }> }) {
  const [{ key }, client, me, extensions] = await Promise.all([
    params,
    sessionClient(),
    requireStaff(),
    loadExtensions(),
  ]);
  const app = loaded(await client.GET("/admin/apps/{key}", { params: { path: { key } } }));
  const builtIn = isBuiltIn(app);
  // A spore's settings (design note 0030, §8); one that takes none answers 404.
  const settings = builtIn
    ? (await client.GET("/admin/apps/{key}/settings", { params: { path: { key } } })).data
    : undefined;
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
            canWrite &&
            (builtIn ? (
              <SporeSwitch appKey={app.key} isEnabled={app.enabled} />
            ) : (
              <AppActions
                appKey={app.key}
                name={app.name}
                isEnabled={app.enabled}
                isInstalled={app.installed}
                isUninstalled={Boolean(app.uninstalled_at)}
              />
            ))
          }
        />

        <AppPageTabs appKey={app.key} pages={extensions.pages.filter((p) => p.app === app.key)} />

        {settings && (
          <Panel
            title="Settings"
            description="What this store sets for it. Secret fields are kept sealed and never shown again."
          >
            <SporeSettingsForm
              appKey={app.key}
              schema={settings.schema}
              settings={settings.settings}
              canWrite={canWrite}
            />
          </Panel>
        )}

        <Panel
          title="Access"
          description={
            builtIn
              ? "What it acts with, as declared in the build. Turn it off to stop it; its access can't be narrowed."
              : app.grant
                ? "What the app's tokens may do, at most: approved by staff when it was installed."
                : "This app has no OAuth grant: it can't call the API, only receive webhooks."
          }
          action={
            canWrite &&
            !builtIn &&
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

        {!builtIn && (
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
        )}

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
