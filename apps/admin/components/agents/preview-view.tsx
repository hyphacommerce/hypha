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

function Change({ event, names }: { event: PreviewEvent; names: Record<string, string> }) {
  const parts = eventSentence(event.type, event.data);
  return (
    <div className="change-row">
      <VStack gap={1}>
        {parts ? (
          <Sentence parts={parts} names={names} />
        ) : (
          <Text>
            Records <Text type="code">{event.type}</Text> about{" "}
            <RecordLink id={event.subject} names={names} />
          </Text>
        )}
        <JsonDisclosure label={`${event.type} v${event.version}`} value={event.data} />
      </VStack>
    </div>
  );
}

/**
 * The preview's parts as cards. `names` are records the page could name (locations);
 * the response's own record is named from its body.
 */
export function PreviewView({
  preview,
  names = NO_NAMES,
}: {
  preview: Preview;
  names?: Record<string, string>;
}) {
  const named = previewNames(preview, names);
  return (
    <VStack gap={5}>
      {!preview.complete && (
        <Banner
          status="warning"
          title="Only part of this could be previewed"
          description={`${preview.blocked_by ? blockedLabel(preview.blocked_by) : "It stopped at an effect a preview can't run."} What it did up to there is shown; approving runs it in full, including that step.`}
        />
      )}

      <PreviewCard
        title="What would change"
        description="The changes the write would record, in order. Nothing has changed yet."
      >
        {preview.events.length === 0 ? (
          <Text color="secondary">No recorded changes.</Text>
        ) : (
          <div>
            {keyed(preview.events, (e) => `${e.type}:${e.subject}:${JSON.stringify(e.data)}`).map(
              ({ key, item }) => (
                <Change key={key} event={item} names={named} />
              ),
            )}
          </div>
        )}
      </PreviewCard>

      {preview.emails.length > 0 && (
        <PreviewCard title="Emails it would send" description="Sent while answering the write.">
          <VStack gap={2}>
            {keyed(preview.emails, (e) => `${e.template}:${e.to ?? ""}:${e.subject}`).map(
              ({ key, item: email }) => (
                <HStack key={key} gap={2} vAlign="center" wrap="wrap">
                  <Badge label={email.template} />
                  <Text weight="medium">{email.subject}</Text>
                  <Text color="secondary">
                    to {email.to ?? "a customer (address hidden from this agent)"}
                  </Text>
                </HStack>
              ),
            )}
          </VStack>
        </PreviewCard>
      )}

      {preview.calls.length > 0 && (
        <PreviewCard
          title="Apps it called"
          description="Read-only calls, made for real while previewing (shipping rates, search, a payment's status)."
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

      <PreviewCard title="What it would answer">
        {preview.response ? (
          <VStack gap={2}>
            <HStack gap={2} vAlign="center">
              <Text color="secondary">Status</Text>
              <Badge
                label={String(preview.response.status)}
                variant={preview.response.status < 300 ? "success" : "warning"}
              />
            </HStack>
            <JsonDisclosure label="Response body" value={preview.response.body} />
          </VStack>
        ) : (
          <Text color="secondary">Nothing: it stopped before answering.</Text>
        )}
      </PreviewCard>
    </VStack>
  );
}
