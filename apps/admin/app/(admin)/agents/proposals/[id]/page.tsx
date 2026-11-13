import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { Card } from "@astryxdesign/core/Card";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { MetadataList, MetadataListItem } from "@astryxdesign/core/MetadataList";
import { Section } from "@astryxdesign/core/Section";
import { Text } from "@astryxdesign/core/Text";
import { type ReactNode, Suspense } from "react";

import { JsonDisclosure, type PreviewState, PreviewView } from "@/components/agents/preview-view";
import { DateTime } from "@/components/date-time";
import { type Crumb, PageHeader } from "@/components/page-header";
import { DetailSkeleton } from "@/components/skeletons";
import { getFormat } from "@/lib/get-format";
import { type Proposal, operationLabel, proposalStatus, readPreview, shortId } from "@/lib/agents";
import {
  isMissingScope,
  loaded,
  locationNames,
  requireStaff,
  sessionClient,
  staffNames,
  describeWith,
} from "@/lib/hypha";
import { getT } from "@/lib/i18n";
import type { T } from "@/lib/messages";
import { agentsCrumb, hasScope } from "@/lib/navigation";

import { Decision } from "./decision";

/** The way back: the agents, then the proposals inbox. */
function back(t: T): Crumb[] {
  return [agentsCrumb(t), { label: t("agents.proposals_title"), href: "/agents/proposals" }];
}

/**
 * One proposal: who proposed it and for whom, the write it keeps, and its preview as a
 * change list; Approve and Reject while it's pending, how it was decided after (read-
 * only). The way back renders at once; the proposal streams in.
 */
export default async function ProposalPage({ params }: { params: Promise<{ id: string }> }) {
  const [{ id }, t] = await Promise.all([params, getT()]);
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={1280}>
        <Suspense fallback={<DetailSkeleton back={back(t)} />}>
          <ProposalDetail id={id} />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function ProposalDetail({ id }: { id: string }) {
  const client = await sessionClient();
  const [t, format, proposal, me, people, locations] = await Promise.all([
    getT(),
    getFormat(),
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
  const status = proposalStatus(t, proposal.status);
  const agentLink = (chunks: ReactNode, key: number) => (
    <Link key={key} href={`/agents/${proposal.agent}`}>
      {chunks}
    </Link>
  );
  const proposedAt = <DateTime key="time" value={proposal.created_at} short />;
  const preview = readPreview(proposal.preview);
  const names = Object.fromEntries(locations);
  const missing = proposal.scopes.filter((scope) => !hasScope(me.scopes, scope));

  return (
    <>
      <PageHeader
        title={operationLabel(t, proposal.operation, proposal.summary)}
        back={back(t)}
        badges={<Badge label={status.label} variant={status.variant} />}
        description={
          person
            ? t.rich("agents.proposal_proposed_by_for", {
                agent: agent?.name ?? shortId(proposal.agent),
                link: agentLink,
                person,
                time: proposedAt,
              })
            : t.rich("agents.proposal_proposed_by", {
                agent: agent?.name ?? shortId(proposal.agent),
                link: agentLink,
                time: proposedAt,
              })
        }
      />

      <div className="detail-columns">
        <div>
          {proposal.status === "pending" && proposal.refreshes > 0 && (
            <Banner
              status="warning"
              title={t("agents.proposal_stale_title")}
              description={t("agents.proposal_stale_description", { count: proposal.refreshes })}
            />
          )}
          {proposal.status === "pending" ? (
            <Decision id={proposal.id} refreshes={proposal.refreshes} missing={missing} />
          ) : (
            <Outcome t={t} proposal={proposal} people={people} />
          )}
          {proposal.status !== "pending" && (
            <Text type="display-3" as="h2">
              {proposal.status === "applied"
                ? t("agents.proposal_as_applied")
                : t("agents.proposal_as_proposed")}
            </Text>
          )}
          {preview ? (
            <PreviewView
              t={t}
              format={format}
              preview={preview}
              names={names}
              state={previewState(proposal.status)}
            />
          ) : (
            <JsonDisclosure label={t("agents.proposal_preview")} value={proposal.preview} isOpen />
          )}
        </div>
        <div>
          <Card>
            <VStack gap={4}>
              <Text type="display-3" as="h2">
                {t("agents.proposal_card_title")}
              </Text>
              <MetadataList>
                <MetadataListItem label={t("agents.proposal_operation")}>
                  <Text type="code">{proposal.operation}</Text>
                </MetadataListItem>
                <MetadataListItem label={t("agents.proposal_agent")}>
                  <Link href={`/agents/${proposal.agent}`}>
                    {agent?.name ?? shortId(proposal.agent)}
                  </Link>
                </MetadataListItem>
                {person && (
                  <MetadataListItem label={t("agents.proposal_acts_for")}>
                    {person}
                  </MetadataListItem>
                )}
                <MetadataListItem label={t("agents.proposal_proposed")}>
                  <DateTime value={proposal.created_at} />
                </MetadataListItem>
                {proposal.status === "pending" && (
                  <MetadataListItem label={t("agents.proposal_expires")}>
                    <DateTime value={proposal.expires_at} />
                  </MetadataListItem>
                )}
                <MetadataListItem label={t("agents.proposal_approving_needs")}>
                  <HStack gap={1} wrap="wrap">
                    {proposal.scopes.length === 0 ? (
                      <Text color="secondary">{t("agents.proposal_no_scope")}</Text>
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
                  {t("agents.proposal_request_title")}
                </Text>
                <Text type="supporting" color="secondary">
                  {t("agents.proposal_request_description")}
                </Text>
              </VStack>
              {proposal.request && (
                <JsonDisclosure
                  label={t("agents.proposal_request_path")}
                  value={{ ...proposal.request.params, ...proposal.request.query }}
                  isOpen
                />
              )}
              {proposal.request?.body != null && (
                <JsonDisclosure
                  label={t("agents.proposal_request_body")}
                  value={proposal.request.body}
                  isOpen
                />
              )}
            </VStack>
          </Card>
        </div>
      </div>
    </>
  );
}

/** Where a proposal's write stands, for how its preview speaks of it. */
function previewState(status: Proposal["status"]): PreviewState {
  switch (status) {
    case "pending":
      return "pending";
    case "applied":
      return "applied";
    default:
      return "not_applied";
  }
}

/** How a decided proposal ended: applied, failed (stale or refused), rejected or expired. */
function Outcome({
  t,
  proposal,
  people,
}: {
  t: T;
  proposal: Proposal;
  people: Map<string, string>;
}) {
  const decider = proposal.decided_by
    ? (people.get(proposal.decided_by) ?? shortId(proposal.decided_by))
    : undefined;
  const when = proposal.decided_at ? (
    <DateTime key="time" value={proposal.decided_at} short />
  ) : null;
  const decided = when && (
    <Text color="secondary">{t.rich("agents.outcome_decided", { time: when })}</Text>
  );
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
              title={t("agents.outcome_applied_title")}
              description={
                decider
                  ? t("agents.outcome_applied_by", { person: decider })
                  : t("agents.outcome_applied")
              }
            />
            {decided}
            {body && <JsonDisclosure label={t("agents.outcome_answer")} value={body} />}
          </VStack>
        </Card>
      );
    case "failed":
      return (
        <Card>
          <VStack gap={3}>
            <Banner
              status="error"
              title={t("agents.outcome_failed_title")}
              description={
                !message
                  ? t("agents.outcome_failed_description")
                  : code
                    ? t("agents.outcome_failed_reason", {
                        reason: describeWith(t, { code, error: message, details: body?.details }),
                        code,
                      })
                    : message
              }
            />
            {decided}
            {body && <JsonDisclosure label={t("agents.outcome_answer")} value={body} />}
          </VStack>
        </Card>
      );
    case "rejected":
      return (
        <Card>
          <VStack gap={2}>
            <Banner
              status="info"
              title={t("agents.outcome_rejected_title")}
              description={
                decider
                  ? t("agents.outcome_rejected_by", { person: decider })
                  : t("agents.outcome_rejected")
              }
            />
            {decided}
            {proposal.note && (
              <VStack gap={1}>
                <Text weight="medium">{t("agents.outcome_note")}</Text>
                <Text>{proposal.note}</Text>
              </VStack>
            )}
          </VStack>
        </Card>
      );
    case "applying":
      return (
        <Banner
          status="info"
          title={t("agents.outcome_applying_title")}
          description={
            decider
              ? t("agents.outcome_applying_by", { person: decider })
              : t("agents.outcome_applying")
          }
        />
      );
    case "expired":
      return (
        <Banner
          status="info"
          title={t("agents.outcome_expired_title")}
          description={t("agents.outcome_expired_description")}
        />
      );
    default:
      return (
        <Banner status="info" title={t("agents.outcome_other", { status: proposal.status })} />
      );
  }
}
