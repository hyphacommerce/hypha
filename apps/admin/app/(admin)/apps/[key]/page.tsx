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
import { type ReactNode, Suspense } from "react";

import { DateTime } from "@/components/date-time";
import { type Crumb, PageHeader } from "@/components/page-header";
import { FormSkeleton } from "@/components/skeletons";
import { appState, isBuiltIn, isPersonalDataScope, personalDataLabel } from "@/lib/apps";
import { loadExtensions } from "@/lib/app-extensions";
import { getFormat } from "@/lib/get-format";
import { hasKey } from "@/lib/keys";
import { loaded, requireStaff, sessionClient } from "@/lib/hypha";
import { getT } from "@/lib/i18n";
import type { T } from "@/lib/messages";

import { AppActions } from "./app-actions";
import { AppPageTabs } from "./app-pages";
import { ReduceAccess } from "./reduce-access";
import { SporeSettingsForm } from "./spore-settings";
import { SporeSwitch } from "./spore-switch";

const HISTORY_VARIANT: Record<string, "success" | "warning" | "error"> = {
  pending: "warning",
  approved: "success",
  denied: "error",
};

/** An authorization's status in words; one the admin doesn't know shows as it is. */
function historyStatus(t: T, status: string): string {
  const key = `apps.history_status_${status}`;
  return hasKey(t, key) ? t(key) : status;
}

/** The way back to the list of apps. */
function back(t: T): Crumb {
  return { label: t("common.apps"), href: "/apps" };
}

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
 * One app: what it may do, which personal data it sees, where its events go, and how it was authorized. A spore
 * built into the API shows its switch and its settings form instead of uninstalling and webhooks. The app's own
 * pages (`apps.page`, `settings.app`) are linked above, each at `/apps/{key}/pages/{id}`. The way back renders at
 * once; the app streams in behind a skeleton of the page.
 */
export default async function AppPage({ params }: { params: Promise<{ key: string }> }) {
  const [{ key }, t] = await Promise.all([params, getT()]);
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={960}>
        <Suspense
          fallback={
            <>
              <PageHeader back={back(t)} />
              <FormSkeleton cards={2} />
            </>
          }
        >
          <App appKey={key} />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function App({ appKey: key }: { appKey: string }) {
  const [t, format, client, me, extensions] = await Promise.all([
    getT(),
    getFormat(),
    sessionClient(),
    requireStaff(),
    loadExtensions(),
  ]);
  const app = loaded(await client.GET("/admin/apps/{key}", { params: { path: { key } } }));
  const builtIn = isBuiltIn(app);
  // A spore's settings; one that takes none answers 404.
  const settings = builtIn
    ? (await client.GET("/admin/apps/{key}/settings", { params: { path: { key } } })).data
    : undefined;
  const state = appState(t, app);
  const scopes = app.grant?.scopes ?? [];
  const personal = scopes.filter(isPersonalDataScope);
  const other = scopes.filter((scope) => !isPersonalDataScope(scope));
  const canWrite = me.scopes.includes("apps:write");

  return (
    <>
      <PageHeader
        title={app.name}
        back={back(t)}
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

      <AppPageTabs
        t={t}
        appKey={app.key}
        pages={extensions.pages.filter((p) => p.app === app.key)}
      />

      {settings && (
        <Panel title={t("common.settings")} description={t("apps.detail_settings_description")}>
          <SporeSettingsForm
            appKey={app.key}
            schema={settings.schema}
            settings={settings.settings}
            canWrite={canWrite}
          />
        </Panel>
      )}

      <Panel
        title={t("apps.detail_access_title")}
        description={
          builtIn
            ? t("apps.detail_access_built_in")
            : app.grant
              ? t("apps.detail_access_granted")
              : t("apps.detail_access_none")
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
                <Text color="secondary">{t("apps.detail_no_store_scopes")}</Text>
              ) : (
                other.map((scope) => <Badge key={scope} label={scope} />)
              )}
            </HStack>
            <Text type="supporting" color="secondary">
              {t.rich("apps.detail_approved_by", {
                actor: app.grant.approved_by?.type ?? "unknown",
                time: <DateTime key="time" value={app.grant.approved_at} short />,
              })}
            </Text>
          </VStack>
        ) : (
          <Text color="secondary">{t("apps.detail_custom_hint")}</Text>
        )}
      </Panel>

      <Panel
        title={t("apps.detail_personal_title")}
        description={
          builtIn ? t("apps.detail_personal_built_in") : t("apps.detail_personal_granted")
        }
      >
        {personal.length === 0 ? (
          <Text color="secondary">
            {builtIn ? t("apps.detail_personal_none_built_in") : t("apps.detail_personal_none")}
          </Text>
        ) : (
          <VStack gap={2}>
            {personal.map((scope) => (
              <HStack key={scope} gap={2} vAlign="center">
                <Badge label={scope} variant="warning" />
                <Text>{personalDataLabel(t, scope)}</Text>
              </HStack>
            ))}
          </VStack>
        )}
      </Panel>

      {!builtIn && (
        <Panel
          title={t("apps.detail_webhooks")}
          description={
            app.webhook
              ? t("apps.detail_webhooks_to", { url: app.webhook.url })
              : t("apps.detail_webhooks_none")
          }
          action={
            app.webhook && (
              <Button
                label={t("apps.detail_see_deliveries")}
                variant="secondary"
                href={`/apps/${app.key}/deliveries`}
              />
            )
          }
        >
          {app.webhook &&
            (app.subscriptions.length === 0 ? (
              <Badge label={t("apps.dev_no_events")} variant="warning" />
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
        <MetadataList title={t("apps.detail_installed_from")}>
          <MetadataListItem label={t("apps.detail_manifest")}>
            <Text type="code">{app.manifest.url}</Text>
          </MetadataListItem>
          <MetadataListItem label={t("apps.detail_manifest_read")}>
            <DateTime value={app.manifest.fetched_at} short />
          </MetadataListItem>
          <MetadataListItem label={t("apps.detail_added")}>
            <DateTime value={app.created_at} short />
          </MetadataListItem>
        </MetadataList>
      )}

      {app.history.length > 0 && (
        <Panel
          title={t("apps.detail_history_title")}
          description={t("apps.detail_history_description")}
        >
          <Table>
            <TableHeader>
              <TableRow>
                <TableHeaderCell>{t("apps.detail_history_asked")}</TableHeaderCell>
                <TableHeaderCell>{t("apps.detail_history_scopes")}</TableHeaderCell>
                <TableHeaderCell>{t("apps.detail_history_answer")}</TableHeaderCell>
              </TableRow>
            </TableHeader>
            <TableBody>
              {app.history.map((entry) => {
                return (
                  <TableRow key={entry.id}>
                    <TableCell>
                      <DateTime value={entry.created_at} short />
                    </TableCell>
                    <TableCell>
                      <Text type="supporting">
                        {entry.scopes.length > 0
                          ? format.list(entry.scopes, "unit")
                          : t("common.none")}
                      </Text>
                    </TableCell>
                    <TableCell>
                      <VStack gap={0}>
                        <Badge
                          label={historyStatus(t, entry.status)}
                          variant={HISTORY_VARIANT[entry.status] ?? "warning"}
                        />
                        {entry.decided_at && (
                          <Text type="supporting" color="secondary">
                            {t("apps.detail_history_by", {
                              actor: entry.decided_by?.type ?? "unknown",
                            })}
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
    </>
  );
}
