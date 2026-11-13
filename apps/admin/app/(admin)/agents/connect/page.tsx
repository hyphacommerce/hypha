import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { Card } from "@astryxdesign/core/Card";
import { EmptyState } from "@astryxdesign/core/EmptyState";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { Section } from "@astryxdesign/core/Section";
import { Text } from "@astryxdesign/core/Text";
import { CpuChipIcon, ExclamationTriangleIcon } from "@heroicons/react/24/outline";
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { Suspense } from "react";

import { DateTime } from "@/components/date-time";
import { type Crumb, PageHeader } from "@/components/page-header";
import { FormSkeleton } from "@/components/skeletons";
import { describe, requireStaff, sessionClient } from "@/lib/hypha";
import { getT } from "@/lib/i18n";
import type { T } from "@/lib/messages";

import { draftOf } from "../grant";
import { ConnectForm } from "./connect-form";

type Connection = AdminSchemas["AgentConnectionResponse"];

/** The way back to the list of agents. */
function back(t: T): Crumb {
  return { label: t("agents.list_title"), href: "/agents" };
}

/** Where a client starts connecting: a command, never translated. */
const MCP_COMMAND = "/mcp";

/**
 * The consent page for an MCP client (Claude Code, Cursor, ChatGPT…): where the API's
 * `/oauth/authorize` sends the browser (`?request=oareq_…`). It shows who the client
 * says it is and, plainly, where approving sends its code (the redirect's host, with a
 * warning when that's only ever this computer), then a name and a grant preset from
 * the suggestion. Connecting makes an agent acting for the signed-in staff member,
 * capped by their role; any staff member may connect their own client. Signed out, the
 * login comes first and returns here (`proxy.ts`).
 */
export default async function ConnectPage({
  searchParams,
}: {
  searchParams: Promise<Record<string, string | string[] | undefined>>;
}) {
  const [{ request: id }, t] = await Promise.all([searchParams, getT()]);
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={960}>
        {typeof id !== "string" || !id.startsWith("oareq_") ? (
          <Problem t={t} title={t("agents.connect_no_request_title")} />
        ) : (
          <Suspense
            fallback={
              <>
                <PageHeader back={back(t)} />
                <FormSkeleton cards={2} />
              </>
            }
          >
            <ConnectRequest id={id} />
          </Suspense>
        )}
      </VStack>
    </Section>
  );
}

async function ConnectRequest({ id }: { id: string }) {
  const [client, me, t] = await Promise.all([sessionClient(), requireStaff(), getT()]);
  const result = await client.GET("/admin/agent-connections/{id}", { params: { path: { id } } });
  if (!result.data) {
    return result.response.status === 404 ? (
      <Problem t={t} title={t("agents.connect_missing_title")} />
    ) : (
      <Problem
        t={t}
        title={t("agents.connect_unanswerable_title")}
        description={result.error ? await describe(result.error) : t("agents.connect_again")}
      />
    );
  }
  const request = result.data;
  const { client: agentClient } = request;

  return (
    <>
      <PageHeader
        title={t("agents.connect_title", { name: agentClient.name })}
        back={back(t)}
        badges={<StatusBadge t={t} status={request.status} />}
        description={t.rich("agents.connect_description", {
          name: me.user.name,
          time: <DateTime key="time" value={request.created_at} short />,
        })}
      />

      <ClientIdentity t={t} request={request} />

      {request.status === "pending" ? (
        <ConnectForm
          id={request.id}
          clientName={agentClient.name}
          redirectHost={agentClient.redirect_host}
          grant={draftOf(request.suggested_grant)}
          requested={request.requested_scopes}
          cap={request.writes_cap}
          myScopes={me.scopes}
        />
      ) : (
        <Banner
          status={request.status === "expired" ? "warning" : "info"}
          title={
            request.status === "approved"
              ? t("agents.connect_already_connected")
              : request.status === "denied"
                ? t("agents.connect_already_declined")
                : t("agents.connect_request_expired")
          }
          description={
            request.status === "expired"
              ? t("agents.connect_expired_description")
              : t("agents.connect_answered_once")
          }
          endContent={
            request.status === "approved" ? (
              <Link href="/agents">{t("agents.connect_open_agents")}</Link>
            ) : undefined
          }
        />
      )}
    </>
  );
}

/**
 * Who the client says it is, and where its access goes. A name and a logo are anyone's
 * to pick, so the redirect's host is the fact that matters.
 */
function ClientIdentity({ t, request }: { t: T; request: Connection }) {
  const { client } = request;
  return (
    <Card>
      <VStack gap={4}>
        <HStack gap={4} vAlign="center">
          {client.logo_uri ? (
            <img
              src={client.logo_uri}
              alt=""
              width={56}
              height={56}
              style={{ borderRadius: 12, objectFit: "cover" }}
            />
          ) : (
            <CpuChipIcon width={56} />
          )}
          <VStack gap={1}>
            <Text type="display-3" as="h2">
              {client.name}
            </Text>
            {client.client_uri && (
              <Link href={client.client_uri} isExternalLink>
                {hostOf(client.client_uri) ?? client.client_uri}
              </Link>
            )}
            <Text type="supporting" color="secondary">
              {client.client_id}
            </Text>
          </VStack>
        </HStack>
        <Text>
          {t.rich("agents.connect_go_back_to", {
            host: (
              <Text key="host" weight="semibold" type="inherit">
                {client.redirect_host}
              </Text>
            ),
          })}
        </Text>
        {client.loopback_only && (
          <Banner
            status="warning"
            title={t("agents.connect_loopback_title")}
            description={t("agents.connect_loopback_description", {
              host: client.redirect_host,
              name: client.name,
            })}
          />
        )}
      </VStack>
    </Card>
  );
}

function hostOf(url: string): string | undefined {
  try {
    return new URL(url).host;
  } catch {
    return undefined;
  }
}

function Problem({ t, title, description }: { t: T; title: string; description?: string }) {
  return (
    <EmptyState
      icon={<ExclamationTriangleIcon width={32} />}
      title={title}
      description={description ?? t("agents.connect_problem_description", { command: MCP_COMMAND })}
      actions={<Link href="/agents">{t("agents.connect_back_to_agents")}</Link>}
    />
  );
}

function StatusBadge({ t, status }: { t: T; status: string }) {
  switch (status) {
    case "pending":
      return <Badge label={t("agents.connect_status_pending")} variant="info" />;
    case "approved":
      return <Badge label={t("agents.connect_status_approved")} variant="success" />;
    case "denied":
      return <Badge label={t("agents.connect_status_denied")} variant="error" />;
    case "expired":
      return <Badge label={t("agents.connect_status_expired")} variant="warning" />;
    default:
      return <Badge label={status} />;
  }
}
