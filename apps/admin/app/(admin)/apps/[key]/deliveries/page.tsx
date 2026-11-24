import { Badge } from "@astryxdesign/core/Badge";
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
import { InboxIcon } from "@heroicons/react/24/outline";
import { redirect } from "next/navigation";
import { Suspense } from "react";

import { DateTime } from "@/components/date-time";
import { PageHeader } from "@/components/page-header";
import { Pager } from "@/components/pager";
import { TableSkeleton } from "@/components/skeletons";

import { loaded, sessionClient } from "@/lib/hypha";
import { getT } from "@/lib/i18n";
import { labelOr } from "@/lib/keys";
import type { T } from "@/lib/messages";
import { appsCrumb } from "@/lib/navigation";

import { RetryButton } from "./retry-button";

const STATUSES: Record<string, "success" | "warning" | "error"> = {
  pending: "warning",
  succeeded: "success",
  failed: "error",
};

/** A delivery's status in words; one the admin doesn't know shows as it is. */
function statusLabel(t: T, status: string): string {
  const key = `apps.delivery_status_${status}`;
  return labelOr(t, key, status);
}

/**
 * An app's webhook deliveries, newest event first. The title and the way back render at
 * once; the app (which names the crumb above this page) and its deliveries stream in
 * behind a skeleton of the table.
 */
export default async function AppDeliveriesPage({
  params,
  searchParams,
}: {
  params: Promise<{ key: string }>;
  searchParams: Promise<Record<string, string | string[] | undefined>>;
}) {
  const [{ key }, query, t] = await Promise.all([params, searchParams, getT()]);
  const cursor = typeof query.cursor === "string" ? query.cursor : undefined;
  const status =
    typeof query.status === "string" && query.status in STATUSES ? query.status : undefined;

  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={960}>
        <Suspense
          fallback={
            <>
              <PageHeader title={t("apps.deliveries_title")} back={appsCrumb(t)} />
              <TableSkeleton />
            </>
          }
        >
          <Deliveries appKey={key} cursor={cursor} status={status} />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function Deliveries({
  appKey: key,
  cursor,
  status,
}: {
  appKey: string;
  cursor: string | undefined;
  status: string | undefined;
}) {
  const path = `/apps/${key}/deliveries`;

  const client = await sessionClient();
  const [app, result, t] = await Promise.all([
    client.GET("/admin/apps/{key}", { params: { path: { key } } }).then(loaded),
    client.GET("/admin/apps/{key}/deliveries", {
      params: { path: { key }, query: { cursor, status } },
    }),
    getT(),
  ]);
  // A hand-edited URL (an old or foreign cursor): start the list again.
  if (result.response.status === 400) redirect(path);
  const page = loaded(result);

  return (
    <>
      <PageHeader
        title={t("apps.deliveries_title")}
        back={[appsCrumb(t), { label: app.name, href: `/apps/${key}` }]}
        description={
          app.webhook
            ? t("apps.deliveries_description", { url: app.webhook.url })
            : t("apps.deliveries_no_webhook")
        }
      />
      <HStack gap={3}>
        <Link href={path} weight={status ? "normal" : "medium"}>
          {t("common.all")}
        </Link>
        {Object.keys(STATUSES).map((value) => (
          <Link
            key={value}
            href={`${path}?status=${value}`}
            weight={status === value ? "medium" : "normal"}
          >
            {statusLabel(t, value)}
          </Link>
        ))}
      </HStack>

      {page.items.length === 0 ? (
        <EmptyState
          icon={<InboxIcon width={32} />}
          title={cursor ? t("apps.deliveries_empty_more") : t("apps.deliveries_empty")}
          description={t("apps.deliveries_empty_description")}
        />
      ) : (
        <Table>
          <TableHeader>
            <TableRow>
              <TableHeaderCell>{t("apps.deliveries_event")}</TableHeaderCell>
              <TableHeaderCell>{t("apps.deliveries_occurred")}</TableHeaderCell>
              <TableHeaderCell>{t("common.status")}</TableHeaderCell>
              <TableHeaderCell>
                <span />
              </TableHeaderCell>
            </TableRow>
          </TableHeader>
          <TableBody>
            {page.items.map((delivery) => (
              <TableRow key={delivery.id}>
                <TableCell>
                  <VStack gap={0}>
                    <Text type="code">{delivery.event_type}</Text>
                    <Text type="supporting" color="secondary">
                      {delivery.event_id}
                    </Text>
                  </VStack>
                </TableCell>
                <TableCell>
                  <DateTime value={delivery.occurred_at} short />
                </TableCell>
                <TableCell>
                  <VStack gap={1}>
                    <HStack gap={1}>
                      <Badge
                        label={statusLabel(t, delivery.status)}
                        variant={STATUSES[delivery.status] ?? "warning"}
                      />
                      <Text type="supporting" color="secondary">
                        {t("apps.deliveries_attempts", { count: delivery.attempts })}
                      </Text>
                    </HStack>
                    {delivery.last_error && (
                      <Text type="supporting" color="secondary">
                        {delivery.last_error}
                      </Text>
                    )}
                    {delivery.status === "pending" && delivery.attempts > 0 && (
                      <Text type="supporting" color="secondary">
                        {t.rich("apps.deliveries_next_try", {
                          time: <DateTime key="time" value={delivery.next_attempt_at} short />,
                        })}
                      </Text>
                    )}
                  </VStack>
                </TableCell>
                <TableCell>
                  {delivery.status !== "succeeded" && <RetryButton appKey={key} id={delivery.id} />}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}

      <Pager
        path={path}
        query={{ status }}
        isFirstPage={!cursor}
        nextCursor={page.next_cursor}
        t={t}
      />
    </>
  );
}
