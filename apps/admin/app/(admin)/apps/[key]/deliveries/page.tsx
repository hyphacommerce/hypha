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

import { DateTime } from "@/components/date-time";
import { PageHeader } from "@/components/page-header";
import { Pager } from "@/components/pager";

import { loaded, sessionClient } from "@/lib/hypha";

import { RetryButton } from "./retry-button";

const STATUSES: Record<string, { label: string; variant: "success" | "warning" | "error" }> = {
  pending: { label: "Pending", variant: "warning" },
  succeeded: { label: "Delivered", variant: "success" },
  failed: { label: "Failed", variant: "error" },
};

/** An app's webhook deliveries, newest event first. */
export default async function AppDeliveriesPage({
  params,
  searchParams,
}: {
  params: Promise<{ key: string }>;
  searchParams: Promise<Record<string, string | string[] | undefined>>;
}) {
  const [{ key }, query] = await Promise.all([params, searchParams]);
  const cursor = typeof query.cursor === "string" ? query.cursor : undefined;
  const status =
    typeof query.status === "string" && query.status in STATUSES ? query.status : undefined;
  const path = `/apps/${key}/deliveries`;

  const client = await sessionClient();
  const [app, result] = await Promise.all([
    client.GET("/admin/apps/{key}", { params: { path: { key } } }).then(loaded),
    client.GET("/admin/apps/{key}/deliveries", {
      params: { path: { key }, query: { cursor, status } },
    }),
  ]);
  // A hand-edited URL (an old or foreign cursor): start the list again.
  if (result.response.status === 400) redirect(path);
  const page = loaded(result);

  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={960}>
        <PageHeader
          title="Webhook deliveries"
          back={[
            { label: "Apps", href: "/apps" },
            { label: app.name, href: `/apps/${key}` },
          ]}
          description={
            app.webhook
              ? `Events go to ${app.webhook.url}. Failed ones are retried for about 3 days.`
              : "This app has no webhook, so it gets no events."
          }
        />
        <HStack gap={3}>
          <Link href={path} weight={status ? "normal" : "medium"}>
            All
          </Link>
          {Object.entries(STATUSES).map(([value, { label }]) => (
            <Link
              key={value}
              href={`${path}?status=${value}`}
              weight={status === value ? "medium" : "normal"}
            >
              {label}
            </Link>
          ))}
        </HStack>

        {page.items.length === 0 ? (
          <EmptyState
            icon={<InboxIcon width={32} />}
            title={cursor ? "No more deliveries" : "No deliveries"}
            description="Each event the app subscribes to shows up here, kept for 30 days."
          />
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHeaderCell>Event</TableHeaderCell>
                <TableHeaderCell>Occurred</TableHeaderCell>
                <TableHeaderCell>Status</TableHeaderCell>
                <TableHeaderCell>
                  <span />
                </TableHeaderCell>
              </TableRow>
            </TableHeader>
            <TableBody>
              {page.items.map((delivery) => {
                const shown = STATUSES[delivery.status] ?? {
                  label: delivery.status,
                  variant: "warning" as const,
                };
                return (
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
                          <Badge label={shown.label} variant={shown.variant} />
                          <Text type="supporting" color="secondary">
                            {delivery.attempts === 1
                              ? "1 attempt"
                              : `${delivery.attempts} attempts`}
                          </Text>
                        </HStack>
                        {delivery.last_error && (
                          <Text type="supporting" color="secondary">
                            {delivery.last_error}
                          </Text>
                        )}
                        {delivery.status === "pending" && delivery.attempts > 0 && (
                          <Text type="supporting" color="secondary">
                            Next try <DateTime value={delivery.next_attempt_at} short />
                          </Text>
                        )}
                      </VStack>
                    </TableCell>
                    <TableCell>
                      {delivery.status !== "succeeded" && (
                        <RetryButton appKey={key} id={delivery.id} />
                      )}
                    </TableCell>
                  </TableRow>
                );
              })}
            </TableBody>
          </Table>
        )}

        <Pager path={path} query={{ status }} isFirstPage={!cursor} nextCursor={page.next_cursor} />
      </VStack>
    </Section>
  );
}
