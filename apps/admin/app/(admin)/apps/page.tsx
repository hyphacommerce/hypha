import { Badge } from "@astryxdesign/core/Badge";
import { Button } from "@astryxdesign/core/Button";
import { EmptyState } from "@astryxdesign/core/EmptyState";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
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
import { LockClosedIcon, PuzzlePieceIcon } from "@heroicons/react/24/outline";
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { Suspense } from "react";

import { PageHeader } from "@/components/page-header";
import { TableSkeleton } from "@/components/skeletons";
import { appState, isBuiltIn, isPersonalDataScope } from "@/lib/apps";
import { loaded, publicApiUrl, requireStaff, sessionClient } from "@/lib/hypha";
import { getT } from "@/lib/i18n";
import type { T } from "@/lib/messages";

import { InstallButtons } from "./install";

/**
 * Apps: everything connected to the store, installed through OAuth or made by hand. Each
 * opens its own page, where the apps platform's pages for it will live too. The header
 * renders at once; the apps stream in.
 */
export default async function AppsPage() {
  const [t, me] = await Promise.all([getT(), requireStaff()]);

  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={960}>
        <PageHeader
          title={t("common.apps")}
          description={t("apps.list_description")}
          actions={
            <>
              <Button
                label={t("apps.list_custom_apps")}
                variant="secondary"
                href="/settings/apps"
              />
              <InstallButtons api={publicApiUrl()} canWrite={me.scopes.includes("apps:write")} />
            </>
          }
        />
        <Suspense fallback={<TableSkeleton rows={4} />}>
          <AppRows />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function AppRows() {
  const [client, t] = await Promise.all([sessionClient(), getT()]);
  const result = await client.GET("/admin/apps");
  if (result.response.status === 403) {
    return (
      <EmptyState
        icon={<LockClosedIcon width={32} />}
        title={t("apps.list_no_access_title")}
        description={t("apps.list_no_access_description", { scope: "apps:read" })}
      />
    );
  }
  return <AppsTable t={t} apps={loaded(result).items} />;
}

function AppsTable({ t, apps }: { t: T; apps: AdminSchemas["AppResponse"][] }) {
  if (apps.length === 0) {
    return (
      <EmptyState
        icon={<PuzzlePieceIcon width={32} />}
        title={t("apps.list_empty_title")}
        description={t("apps.list_empty_description")}
        actions={
          <Button label={t("apps.list_add_custom")} variant="primary" href="/settings/apps" />
        }
      />
    );
  }
  return (
    <Table>
      <TableHeader>
        <TableRow>
          <TableHeaderCell>{t("common.app")}</TableHeaderCell>
          <TableHeaderCell>{t("common.status")}</TableHeaderCell>
          <TableHeaderCell>{t("apps.list_access")}</TableHeaderCell>
        </TableRow>
      </TableHeader>
      <TableBody>
        {apps.map((app) => {
          const state = appState(t, app);
          const scopes = app.grant?.scopes ?? [];
          const personal = scopes.filter(isPersonalDataScope).length;
          return (
            <TableRow key={app.key}>
              <TableCell>
                <VStack gap={0}>
                  <HStack gap={2} vAlign="center">
                    <Link href={`/apps/${app.key}`} weight="medium">
                      {app.name}
                    </Link>
                    {isBuiltIn(app) && <Badge label={t("apps.list_built_in")} />}
                  </HStack>
                  <Text type="supporting" color="secondary">
                    {app.key}
                  </Text>
                </VStack>
              </TableCell>
              <TableCell>
                <Badge label={state.label} variant={state.variant} />
              </TableCell>
              <TableCell>
                <HStack gap={1} wrap="wrap">
                  {isBuiltIn(app) ? (
                    <Text type="supporting" color="secondary">
                      {t("apps.list_scopes_built_in", { count: scopes.length })}
                    </Text>
                  ) : app.grant ? (
                    <Text type="supporting" color="secondary">
                      {t("apps.list_scopes", { count: scopes.length })}
                    </Text>
                  ) : (
                    <Text type="supporting" color="secondary">
                      {t("apps.list_webhooks_only")}
                    </Text>
                  )}
                  {personal > 0 && <Badge label={t("apps.list_personal_data")} variant="warning" />}
                </HStack>
              </TableCell>
            </TableRow>
          );
        })}
      </TableBody>
    </Table>
  );
}
