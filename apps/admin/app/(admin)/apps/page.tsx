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
import type { AdminSchemas } from "@hypha/sdk/admin";
import { Suspense } from "react";

import { PageHeader } from "@/components/page-header";
import { TableSkeleton } from "@/components/skeletons";
import { appState, isBuiltIn, isPersonalDataScope } from "@/lib/apps";
import { loaded, publicApiUrl, requireStaff, sessionClient } from "@/lib/hypha";

import { InstallButtons } from "./install";

/**
 * Apps: everything connected to the store, installed through OAuth or made by hand. Each
 * opens its own page, where the apps platform's pages for it will live too. The header
 * renders at once; the apps stream in.
 */
export default async function AppsPage() {
  const me = await requireStaff();

  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={960}>
        <PageHeader
          title="Apps"
          description="Services connected to your store: what each may do, and what it was sent."
          actions={
            <>
              <Button label="Webhooks and custom apps" variant="secondary" href="/settings/apps" />
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
  const client = await sessionClient();
  const result = await client.GET("/admin/apps");
  if (result.response.status === 403) {
    return (
      <EmptyState
        icon={<LockClosedIcon width={32} />}
        title="You can't see apps"
        description="Your role doesn't include apps:read. Ask the store owner for access."
      />
    );
  }
  return <AppsTable apps={loaded(result).items} />;
}

function AppsTable({ apps }: { apps: AdminSchemas["AppResponse"][] }) {
  if (apps.length === 0) {
    return (
      <EmptyState
        icon={<PuzzlePieceIcon width={32} />}
        title="No apps yet"
        description="Install an app from its address, where you approve what it may do. To connect your own code, create a private app."
        actions={<Button label="Add a custom app" variant="primary" href="/settings/apps" />}
      />
    );
  }
  return (
    <Table>
      <TableHeader>
        <TableRow>
          <TableHeaderCell>App</TableHeaderCell>
          <TableHeaderCell>Status</TableHeaderCell>
          <TableHeaderCell>Access</TableHeaderCell>
        </TableRow>
      </TableHeader>
      <TableBody>
        {apps.map((app) => {
          const state = appState(app);
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
                    {isBuiltIn(app) && <Badge label="Built in" />}
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
                      {scopes.length === 1
                        ? "1 scope, built in"
                        : `${scopes.length} scopes, built in`}
                    </Text>
                  ) : app.grant ? (
                    <Text type="supporting" color="secondary">
                      {scopes.length === 1 ? "1 scope" : `${scopes.length} scopes`}
                    </Text>
                  ) : (
                    <Text type="supporting" color="secondary">
                      Webhooks only
                    </Text>
                  )}
                  {personal > 0 && <Badge label="Personal data" variant="warning" />}
                </HStack>
              </TableCell>
            </TableRow>
          );
        })}
      </TableBody>
    </Table>
  );
}
