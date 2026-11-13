// A write's preview, for a person: what it would change (its events as sentences, the
// JSON for types without one), the emails it would send, the apps it would call, where
// it stopped if it couldn't run to its end, and what it would answer. Renders on the
// server and in client components alike (no hooks).

import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { Card } from "@astryxdesign/core/Card";
import { CodeBlock } from "@astryxdesign/core/CodeBlock";
import { Collapsible } from "@astryxdesign/core/Collapsible";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { Text } from "@astryxdesign/core/Text";
import type { ReactNode } from "react";

import {
  type Part,
  type Preview,
  type PreviewEvent,
  blockedLabel,
  eventSentence,
  previewNames,
  recordHref,
  shortId,
} from "@/lib/agents";
import type { Format } from "@/lib/format";
import type { TFor } from "@/lib/messages";

/** A translator with what a preview is written in: its own words, areas and the vocabulary. */
type Translate = TFor<"agents" | "apps" | "common">;

const NO_NAMES: Record<string, string> = {};

/**
 * `items` with React keys from `keyOf`, a repeat numbered after its first (a preview may
 * record the same change twice; its lists never reorder).
 */
function keyed<T>(items: T[], keyOf: (item: T) => string): { key: string; item: T }[] {
  const seen = new Map<string, number>();
  return items.map((item) => {
    const base = keyOf(item);
    const n = seen.get(base) ?? 0;
    seen.set(base, n + 1);
    return { key: n === 0 ? base : `${base}#${n}`, item };
  });
}

/** JSON, pretty, collapsed behind `label` unless `isOpen`. */
export function JsonDisclosure({
  label,
  value,
  isOpen = false,
}: {
  label: string;
  value: unknown;
  isOpen?: boolean;
}) {
  return (
    <Collapsible trigger={<Text type="supporting">{label}</Text>} defaultIsOpen={isOpen}>
      <CodeBlock
        code={JSON.stringify(value, null, 2)}
        language="json"
        width="100%"
        maxHeight={420}
        size="sm"
        isWrapped
      />
    </Collapsible>
  );
}

function PreviewCard({
  title,
  description,
  children,
}: {
  title: string;
  description?: string;
  children: ReactNode;
}) {
  return (
    <Card>
      <VStack gap={3}>
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
        {children}
      </VStack>
    </Card>
  );
}

/** A record named where the page knows it, linked where it has a page. */
function RecordLink({ id, names }: { id: string; names: Record<string, string> }) {
  const label = names[id] ?? shortId(id);
  const href = recordHref(id);
  return href ? (
    <Link href={href} weight="medium">
      {label}
    </Link>
  ) : (
    <Text weight="medium" type="inherit">
      <span title={id}>{label}</span>
    </Text>
  );
}

function Sentence({ parts, names }: { parts: Part[]; names: Record<string, string> }) {
  return (
    <Text>
      {keyed(parts, (part) => (typeof part === "string" ? part : part.id)).map(({ key, item }) =>
        typeof item === "string" ? item : <RecordLink key={key} id={item.id} names={names} />,
      )}
    </Text>
  );
}

function Change({
  t,
  format,
  event,
  names,
}: {
  t: Translate;
  format: Format;
  event: PreviewEvent;
  names: Record<string, string>;
}) {
  const parts = eventSentence(t, format, event.type, event.data);
  return (
    <div className="change-row">
      <VStack gap={1}>
        {parts ? (
          <Sentence parts={parts} names={names} />
        ) : (
          <Text>
            {t.rich("agents.preview_records", {
              type: (
                <Text key="type" type="code">
                  {event.type}
                </Text>
              ),
              subject: <RecordLink key="subject" id={event.subject} names={names} />,
            })}
          </Text>
        )}
        <JsonDisclosure
          label={t("agents.preview_event_version", {
            type: event.type,
            version: String(event.version),
          })}
          value={event.data}
        />
      </VStack>
    </div>
  );
}

/**
 * Where the write a preview shows stands: still to decide (`pending`, also a plain
 * preview), done (`applied`), or never done (`not_applied`: rejected, expired, failed).
 */
export type PreviewState = "pending" | "applied" | "not_applied";

/**
 * The preview's parts as cards, each speaking of the write by where it stands
 * (`agents.preview_*_<state>`). `names` are records the page could name (locations);
 * the response's own record is named from its body. `state` says whether the write is
 * still to decide, was applied or never was.
 */
export function PreviewView({
  t,
  format,
  preview,
  names = NO_NAMES,
  state = "pending",
}: {
  t: Translate;
  format: Format;
  preview: Preview;
  names?: Record<string, string>;
  state?: PreviewState;
}) {
  const named = previewNames(preview, names);
  return (
    <VStack gap={5}>
      {!preview.complete && (
        <Banner
          status="warning"
          title={t("agents.preview_partial_title")}
          description={t("agents.preview_partial_description", {
            blocked: preview.blocked_by
              ? blockedLabel(t, preview.blocked_by)
              : t("agents.preview_stopped"),
            partial: t(`agents.preview_partial_${state}`),
          })}
        />
      )}

      <PreviewCard
        title={t(`agents.preview_changes_${state}`)}
        description={t(`agents.preview_changes_note_${state}`)}
      >
        {preview.events.length === 0 ? (
          <Text color="secondary">{t("agents.preview_no_changes")}</Text>
        ) : (
          <div>
            {keyed(preview.events, (e) => `${e.type}:${e.subject}:${JSON.stringify(e.data)}`).map(
              ({ key, item }) => (
                <Change key={key} t={t} format={format} event={item} names={named} />
              ),
            )}
          </div>
        )}
      </PreviewCard>

      {preview.emails.length > 0 && (
        <PreviewCard
          title={t(`agents.preview_emails_${state}`)}
          description={t("agents.preview_emails_description")}
        >
          <VStack gap={2}>
            {keyed(preview.emails, (e) => `${e.template}:${e.to ?? ""}:${e.subject}`).map(
              ({ key, item: email }) => (
                <HStack key={key} gap={2} vAlign="center" wrap="wrap">
                  <Badge label={email.template} />
                  <Text weight="medium">{email.subject}</Text>
                  <Text color="secondary">
                    {email.to
                      ? t("agents.preview_email_to", { address: email.to })
                      : t("agents.preview_email_to_hidden")}
                  </Text>
                </HStack>
              ),
            )}
          </VStack>
        </PreviewCard>
      )}

      {preview.calls.length > 0 && (
        <PreviewCard
          title={t("agents.preview_calls_title")}
          description={t("agents.preview_calls_description")}
        >
          <VStack gap={2}>
            {keyed(preview.calls, (c) => `${c.point}:${c.app}`).map(({ key, item: call }) => (
              <HStack key={key} gap={2} vAlign="center">
                <Badge label={call.point} />
                <Text>{call.app}</Text>
              </HStack>
            ))}
          </VStack>
        </PreviewCard>
      )}

      <PreviewCard title={t(`agents.preview_answer_${state}`)}>
        {preview.response ? (
          <VStack gap={2}>
            <HStack gap={2} vAlign="center">
              <Text color="secondary">{t("common.status")}</Text>
              <Badge
                label={String(preview.response.status)}
                variant={preview.response.status < 300 ? "success" : "warning"}
              />
            </HStack>
            <JsonDisclosure
              label={t("agents.preview_response_body")}
              value={preview.response.body}
            />
          </VStack>
        ) : (
          <Text color="secondary">{t("agents.preview_no_answer")}</Text>
        )}
      </PreviewCard>
    </VStack>
  );
}
