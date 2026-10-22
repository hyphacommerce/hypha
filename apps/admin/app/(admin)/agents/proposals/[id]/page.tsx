import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { Card } from "@astryxdesign/core/Card";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { MetadataList, MetadataListItem } from "@astryxdesign/core/MetadataList";
import { Section } from "@astryxdesign/core/Section";
import { Text } from "@astryxdesign/core/Text";
import { Suspense } from "react";

import { JsonDisclosure, PreviewView } from "@/components/agents/preview-view";
import { DateTime } from "@/components/date-time";
import { type Crumb, PageHeader } from "@/components/page-header";
import { DetailSkeleton } from "@/components/skeletons";
import {
  type Proposal,
  operationLabel,
  proposalStatus,
  readPreview,
  recordOf,
  shortId,
} from "@/lib/agents";
import {
  isMissingScope,
  loaded,
  locationNames,
  requireStaff,
  sessionClient,
  staffNames,
} from "@/lib/hypha";
import { hasScope } from "@/lib/navigation";

import { Decision } from "./decision";

const BACK: Crumb[] = [
  { label: "Agents", href: "/agents" },
  { label: "Proposals", href: "/agents/proposals" },
];

/**
 * One proposal: who proposed it and for whom, the write it keeps, and its preview as a
 * change list; Approve and Reject while it's pending, how it was decided after (read-
 * only). The way back renders at once; the proposal streams in.
 */
export default async function ProposalPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={1280}>
        <Suspense fallback={<DetailSkeleton back={BACK} />}>
          <ProposalDetail id={id} />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function ProposalDetail({ id }: { id: string }) {
  const client = await sessionClient();
  const [proposal, me, people, locations] = await Promise.all([
    client.GET("/admin/agent-proposals/{id}", { params: { path: { id } } }).then(loaded),
    requireStaff(),
    staffNames(client),
    locationNames(client),
  ]);
  const agentResult = await client.GET("/admin/agents/{id}", {
    params: { path: { id: proposal.agent } },
  });
  // A revoked or deleted agent still names its proposals.
  const agent = agentResult.data;
  if (!agent && !isMissingScope(agentResult) && agentResult.response.status !== 404) {
    loaded(agentResult);
  }
  const person = agent?.user ? (people.get(agent.user) ?? agent.user) : undefined;
  const status = proposalStatus(proposal.status);
  const preview = readPreview(proposal.preview);
  const names = Object.fromEntries(locations);
  const missing = proposal.scopes.filter((scope) => !hasScope(me.scopes, scope));

  return (
    <>
      <PageHeader
        title={operationLabel(proposal.operation)}
        back={BACK}
        badges={<Badge label={status.label} variant={status.variant} />}
        description={
          <>
            Proposed by{" "}
            <Link href={`/agents/${proposal.agent}`}>{agent?.name ?? shortId(proposal.agent)}</Link>
            {person ? ` for ${person}` : ""}, <DateTime value={proposal.created_at} short />
          </>
        }
      />

      <div className="detail-columns">
        <div>
          {proposal.status === "pending" ? (
            <Decision id={proposal.id} missing={missing} />
          ) : (
            <Outcome proposal={proposal} people={people} />
          )}
          {proposal.status !== "pending" && (
            <Text type="display-3" as="h2">
              As proposed
            </Text>
          )}
          {preview ? (
            <PreviewView preview={preview} names={names} />
          ) : (
            <JsonDisclosure label="Preview" value={proposal.preview} isOpen />
          )}
        </div>
        <div>
          <Card>
            <VStack gap={4}>
              <Text type="display-3" as="h2">
                Proposal
              </Text>
              <MetadataList>
                <MetadataListItem label="Operation">
                  <Text type="code">{proposal.operation}</Text>
                </MetadataListItem>
                <MetadataListItem label="Agent">
                  <Link href={`/agents/${proposal.agent}`}>
                    {agent?.name ?? shortId(proposal.agent)}
                  </Link>
                </MetadataListItem>
                {person && <MetadataListItem label="Acts for">{person}</MetadataListItem>}
                <MetadataListItem label="Proposed">
                  <DateTime value={proposal.created_at} />
                </MetadataListItem>
                {proposal.status === "pending" && (
                  <MetadataListItem label="Expires">
                    <DateTime value={proposal.expires_at} />
                  </MetadataListItem>
                )}
                <MetadataListItem label="Approving needs">
                  <HStack gap={1} wrap="wrap">
                    {proposal.scopes.length === 0 ? (
                      <Text color="secondary">No scope</Text>
                    ) : (
                      proposal.scopes.map((scope) => (
                        <Badge
                          key={scope}
                          label={scope}
                          variant={missing.includes(scope) ? "warning" : "neutral"}
                        />
                      ))
                    )}
                  </HStack>
                </MetadataListItem>
              </MetadataList>
            </VStack>
          </Card>
          <Card>
            <VStack gap={3}>
              <VStack gap={1}>
                <Text type="display-3" as="h2">
                  Request
                </Text>
                <Text type="supporting" color="secondary">
                  What approving sends, as the agent made it.
                </Text>
              </VStack>
              <JsonDisclosure
                label="Path and query"
                value={{ ...proposal.request.params, ...proposal.request.query }}
                isOpen
              />
              {proposal.request.body != null && (
                <JsonDisclosure label="Body" value={proposal.request.body} isOpen />
              )}
            </VStack>
          </Card>
        </div>
      </div>
    </>
  );
}

/** How a decided proposal ended: applied, failed (stale or refused), rejected or expired. */
function Outcome({ proposal, people }: { proposal: Proposal; people: Map<string, string> }) {
  const decider = proposal.decided_by
    ? (people.get(proposal.decided_by) ?? shortId(proposal.decided_by))
    : undefined;
  const when = proposal.decided_at ? <DateTime value={proposal.decided_at} short /> : null;
  // The write's answer: Hypha's error body for a failure (`code`, `error`, `details`).
  const body: Record<string, unknown> | undefined = proposal.response?.body;
  const code = typeof body?.code === "string" ? body.code : undefined;
  const message = typeof body?.error === "string" ? body.error : undefined;

  switch (proposal.status) {
    case "applied":
      return (
        <Card>
          <VStack gap={3}>
            <Banner
              status="success"
              title="Applied"
              description={`Approved${decider ? ` by ${decider}` : ""}; the write did what was previewed.`}
            />
            {when && <Text color="secondary">Decided {when}</Text>}
            {body && <JsonDisclosure label="What the write answered" value={body} />}
          </VStack>
        </Card>
      );
    case "failed": {
      const fresh =
        code === "preview_stale" ? readPreview(recordOf(body, "details")?.preview) : null;
      return (
        <Card>
          <VStack gap={3}>
            {code === "preview_stale" ? (
              <Banner
                status="warning"
                title="Not applied: the store changed since this was proposed"
                description={`Approved${decider ? ` by ${decider}` : ""}, but the write would no longer do what was previewed, so nothing changed. Below the original preview is what it would do now. To go ahead, ask the agent to propose it again.`}
              />
            ) : (
              <Banner
                status="error"
                title="Approved, but the write failed"
                description={
                  message
                    ? `${message}${code ? ` (${code})` : ""}`
                    : "The write answered with an error; nothing changed."
                }
              />
            )}
            {when && <Text color="secondary">Decided {when}</Text>}
            {fresh && (
              <VStack gap={3}>
                <Text type="display-3" as="h3">
                  What it would do now
                </Text>
                <PreviewView preview={fresh} />
              </VStack>
            )}
            {body && <JsonDisclosure label="What the write answered" value={body} />}
          </VStack>
        </Card>
      );
    }
    case "rejected":
      return (
        <Card>
          <VStack gap={2}>
            <Banner
              status="info"
              title="Rejected"
              description={`${decider ? `By ${decider}. ` : ""}Nothing was applied.`}
            />
            {when && <Text color="secondary">Decided {when}</Text>}
            {proposal.note && (
              <VStack gap={1}>
                <Text weight="medium">Note for the agent</Text>
                <Text>{proposal.note}</Text>
              </VStack>
            )}
          </VStack>
        </Card>
      );
    case "expired":
      return (
        <Banner
          status="info"
          title="Expired"
          description="Nobody decided it in time, so nothing was applied. The agent can propose it again."
        />
      );
    default:
      return <Banner status="info" title={`This proposal is ${proposal.status}.`} />;
  }
}
