import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { Card } from "@astryxdesign/core/Card";
import { EmptyState } from "@astryxdesign/core/EmptyState";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { Section } from "@astryxdesign/core/Section";
import { Text } from "@astryxdesign/core/Text";
import { CpuChipIcon, ExclamationTriangleIcon } from "@heroicons/react/24/outline";
import type { AdminSchemas } from "@hypha/sdk/admin";
import { Suspense } from "react";

import { DateTime } from "@/components/date-time";
import { type Crumb, PageHeader } from "@/components/page-header";
import { FormSkeleton } from "@/components/skeletons";
import { requireStaff, sessionClient } from "@/lib/hypha";

import { draftOf } from "../grant";
import { ConnectForm } from "./connect-form";

type Connection = AdminSchemas["AgentConnectionResponse"];

const BACK: Crumb = { label: "Agents", href: "/agents" };

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
  const { request: id } = await searchParams;
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={960}>
        {typeof id !== "string" || !id.startsWith("oareq_") ? (
          <Problem title="No request to answer" />
        ) : (
          <Suspense
            fallback={
              <>
                <PageHeader back={BACK} />
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
  const [client, me] = await Promise.all([sessionClient(), requireStaff()]);
  const result = await client.GET("/admin/agent-connections/{id}", { params: { path: { id } } });
  if (!result.data) {
    return result.response.status === 404 ? (
      <Problem title="This request doesn't exist" />
    ) : (
      <Problem
        title="This request can't be answered here"
        description={result.error?.error ?? "Connect again from the client."}
      />
    );
  }
  const request = result.data;
  const { client: agentClient } = request;

  return (
    <>
      <PageHeader
        title={`Connect ${agentClient.name}`}
        back={BACK}
        badges={<StatusBadge status={request.status} />}
        description={
          <>
            It asks to work in this store as an agent acting for you, {me.user.name}. Asked{" "}
            <DateTime value={request.created_at} short />.
          </>
        }
      />

      <ClientIdentity request={request} />

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
              ? "Already connected"
              : request.status === "denied"
                ? "Already declined"
                : "This request expired"
          }
          description={
            request.status === "expired"
              ? "Requests wait ten minutes for an answer. Connect again from the client."
              : "Each request is answered once. To connect again, start from the client."
          }
          endContent={
            request.status === "approved" ? <Link href="/agents">Agents</Link> : undefined
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
function ClientIdentity({ request }: { request: Connection }) {
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
          After you approve, you&apos;ll go back to{" "}
          <Text weight="semibold" type="inherit">
            {client.redirect_host}
          </Text>
          .
        </Text>
        {client.loopback_only && (
          <Banner
            status="warning"
            title="This client only runs on your own computer"
            description={`Its access goes to ${client.redirect_host}, a program on this machine. That's expected for Claude Code, Cursor and other desktop or command-line clients you're connecting right now. If you didn't just start connecting one here, cancel: any program on this computer can call itself ${client.name}.`}
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

function Problem({ title, description }: { title: string; description?: string }) {
  return (
    <EmptyState
      icon={<ExclamationTriangleIcon width={32} />}
      title={title}
      description={
        description ??
        "The link may be old or mistyped. Start connecting again from the client (in Claude Code, /mcp)."
      }
      actions={<Link href="/agents">Back to agents</Link>}
    />
  );
}

function StatusBadge({ status }: { status: string }) {
  switch (status) {
    case "pending":
      return <Badge label="Waiting for you" variant="info" />;
    case "approved":
      return <Badge label="Connected" variant="success" />;
    case "denied":
      return <Badge label="Declined" variant="error" />;
    case "expired":
      return <Badge label="Expired" variant="warning" />;
    default:
      return <Badge label={status} />;
  }
}
