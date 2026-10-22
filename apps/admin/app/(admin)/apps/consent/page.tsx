import { Badge } from "@astryxdesign/core/Badge";
import { Banner } from "@astryxdesign/core/Banner";
import { Card } from "@astryxdesign/core/Card";
import { EmptyState } from "@astryxdesign/core/EmptyState";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { List, ListItem } from "@astryxdesign/core/List";
import { Section } from "@astryxdesign/core/Section";
import { Text } from "@astryxdesign/core/Text";
import { ExclamationTriangleIcon, PuzzlePieceIcon } from "@heroicons/react/24/outline";
import type { AdminSchemas } from "@hypha/sdk/admin";
import type { ReactNode } from "react";

import { DateTime } from "@/components/date-time";
import { PageHeader } from "@/components/page-header";
import { areaLabel, personalDataLabel, scopeLabel } from "@/lib/apps";
import { isMissingScope, requireStaff, sessionClient } from "@/lib/hypha";

import { ConsentButtons } from "./consent-buttons";

type Request = AdminSchemas["ConsentRequestResponse"];

/**
 * The consent page: where Hypha's `/oauth/authorize` sends staff (`?request=oareq_…`),
 * and where an uploaded private app's manifest waits. It shows who the app is, what it
 * asks for in plain words, the personal data it would see with the app's reasons, and
 * what changes compared with its current access; staff who may approve answer, and the
 * browser goes where the API says. Signed out, the login comes first and returns here
 * (`proxy.ts`).
 */
export default async function ConsentPage({
  searchParams,
}: {
  searchParams: Promise<Record<string, string | string[] | undefined>>;
}) {
  const [{ request: id }, client, me] = await Promise.all([
    searchParams,
    sessionClient(),
    requireStaff(),
  ]);
  if (typeof id !== "string" || !id.startsWith("oareq_")) {
    return <Problem title="No request to answer" description={NO_REQUEST} />;
  }
  const result = await client.GET("/admin/oauth/requests/{id}", { params: { path: { id } } });
  if (isMissingScope(result)) {
    return (
      <Problem
        title="You can't answer this"
        description="Your role doesn't include apps:read, so you can't see what the app asks for. Ask the store owner to approve it."
      />
    );
  }
  if (!result.data) {
    return <Problem title="This request doesn't exist" description={NO_REQUEST} />;
  }
  const request = result.data;
  const missing = request.scopes.filter((s) => !me.scopes.includes(s));
  const canWrite = me.scopes.includes("apps:write");
  const blocked = !canWrite
    ? "Your role doesn't include apps:write: only staff who manage apps can approve one."
    : missing.length > 0
      ? `You can only grant what your own role has, and it lacks ${missing.join(", ")}. Ask someone whose role has them.`
      : undefined;

  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={760}>
        <PageHeader
          title={
            request.app.installed
              ? `${request.app.name} asks for more`
              : `Install ${request.app.name}`
          }
          back={{ label: "Apps", href: "/apps" }}
          badges={<StatusBadge status={request.status} />}
          description={
            request.app.private ? (
              "A private app: its manifest was uploaded to this store."
            ) : (
              <>
                Asked <DateTime value={request.created_at} short />
              </>
            )
          }
        />

        <AppIdentity request={request} />

        {request.status === "expired" && (
          <Banner
            status="warning"
            title="This request expired"
            description="Requests wait ten minutes for an answer. Start the installation again from the app."
          />
        )}
        {(request.status === "approved" || request.status === "denied") && (
          <Banner
            status="info"
            title={request.status === "approved" ? "Already approved" : "Already denied"}
            description="Each request is answered once."
            endContent={<Link href={`/apps/${request.app.key}`}>Open the app</Link>}
          />
        )}

        {request.app.installed && <Changes changes={request.changes} />}

        <Card>
          <VStack gap={4}>
            <Text type="display-3" as="h2">
              What it may do
            </Text>
            {request.groups.length === 0 ? (
              <Text color="secondary">Nothing beyond receiving the events it subscribes to.</Text>
            ) : (
              request.groups.map((group) => (
                <VStack key={group.area} gap={1}>
                  <Text type="label">{areaLabel(group.area)}</Text>
                  <List>
                    {group.scopes.map((scope) => (
                      <ListItem
                        key={scope}
                        label={scopeLabel(scope)}
                        description={scope}
                        endContent={
                          request.app.installed && request.changes.added.includes(scope) ? (
                            <Badge label="New" variant="info" />
                          ) : undefined
                        }
                      />
                    ))}
                  </List>
                </VStack>
              ))
            )}
          </VStack>
        </Card>

        <Card>
          <VStack gap={3}>
            <Text type="display-3" as="h2">
              Customers&apos; personal data
            </Text>
            {request.personal_data.length === 0 ? (
              <Text color="secondary">
                None: Hypha leaves names, emails, phone numbers and addresses out of everything this
                app reads or receives.
              </Text>
            ) : (
              <>
                <Text color="secondary">
                  The app will see these fields of your customers. It gives these reasons:
                </Text>
                <List hasDividers>
                  {request.personal_data.map((item) => (
                    <ListItem
                      key={item.scope}
                      label={personalDataLabel(item.scope)}
                      description={`“${item.reason}”`}
                      endContent={<Badge label={item.scope} variant="warning" />}
                    />
                  ))}
                </List>
                <Text type="supporting" color="secondary">
                  What the app receives is its own to keep: read its{" "}
                  <Link href={request.app.privacy_url} isExternalLink>
                    privacy policy
                  </Link>
                  .
                </Text>
              </>
            )}
          </VStack>
        </Card>

        <CustomFields request={request} />

        {/* Always here, so a private app's credentials stay shown once the page re-renders
            with the request approved. */}
        <ConsentButtons
          id={request.id}
          appKey={request.app.key}
          appName={request.app.name}
          isPending={request.status === "pending"}
          canApprove={!blocked}
          canDeny={canWrite}
          intro={
            blocked ? (
              <Banner status="warning" title="You can't approve this" description={blocked} />
            ) : (
              <Text color="secondary">
                {request.app.private
                  ? "Approving installs the app and shows its credentials once."
                  : `Approving sends you back to ${hostOf(request.redirect_uri)}.`}{" "}
                The request waits until <DateTime value={request.expires_at} short />.
              </Text>
            )
          }
        />
      </VStack>
    </Section>
  );
}

const NO_REQUEST =
  "The link may be old or mistyped. Start the installation again from the app, or upload a private app's manifest under Apps.";

function hostOf(url: string): string {
  try {
    return new URL(url).host;
  } catch {
    return "the app";
  }
}

function Problem({ title, description }: { title: string; description: string }) {
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={760}>
        <EmptyState
          icon={<ExclamationTriangleIcon width={32} />}
          title={title}
          description={description}
          actions={<Link href="/apps">Back to apps</Link>}
        />
      </VStack>
    </Section>
  );
}

function StatusBadge({ status }: { status: Request["status"] }) {
  switch (status) {
    case "pending":
      return <Badge label="Waiting for you" variant="info" />;
    case "approved":
      return <Badge label="Approved" variant="success" />;
    case "denied":
      return <Badge label="Denied" variant="error" />;
    case "expired":
      return <Badge label="Expired" variant="warning" />;
  }
}

/** Who the app is: its icon, name, publisher and site. */
function AppIdentity({ request }: { request: Request }) {
  const { app } = request;
  return (
    <Card>
      <HStack gap={4} vAlign="center">
        {app.icon ? (
          <img
            src={app.icon}
            alt=""
            width={56}
            height={56}
            style={{ borderRadius: 12, objectFit: "cover" }}
          />
        ) : (
          <PuzzlePieceIcon width={56} />
        )}
        <VStack gap={1}>
          <HStack gap={2} vAlign="center" wrap="wrap">
            <Text type="display-3" as="h2">
              {app.name}
            </Text>
            <Text type="supporting" color="secondary">
              {app.version}
            </Text>
          </HStack>
          <Text color="secondary">
            {app.publisher ? `By ${app.publisher} · ` : ""}
            <Link href={app.app_url} isExternalLink>
              {hostOf(app.app_url)}
            </Link>
            {" · "}
            <Link href={app.privacy_url} isExternalLink>
              Privacy policy
            </Link>
          </Text>
          <Text type="supporting" color="secondary">
            {app.key}
          </Text>
        </VStack>
      </HStack>
    </Card>
  );
}

function ChangeRow({
  label,
  scopes,
  badge,
}: {
  label: string;
  scopes: string[];
  badge: ReactNode;
}) {
  return (
    scopes.length > 0 && (
      <VStack gap={1}>
        <Text type="label">{label}</Text>
        <HStack gap={1} wrap="wrap">
          {scopes.map((scope) => (
            <HStack key={scope} gap={1} vAlign="center">
              {badge}
              <Text>{scopeLabel(scope)}</Text>
            </HStack>
          ))}
        </HStack>
      </VStack>
    )
  );
}

const OWNER_LABELS: Record<string, string> = {
  product: "Products",
  variant: "Variants",
  category: "Categories",
  customer: "Customers",
  order: "Orders",
};

type Field = Request["custom_fields"]["added"][number];

/** One line per field: its name, where it lives and whether it's about a person. */
function FieldItems({ fields, badge }: { fields: Field[]; badge?: ReactNode }) {
  return (
    <List>
      {fields.map((field) => (
        <ListItem
          key={`${field.owner_type}.${field.key}`}
          label={field.name}
          description={`${OWNER_LABELS[field.owner_type] ?? field.owner_type} · ${field.key} · ${field.type}`}
          endContent={
            <HStack gap={1}>
              {field.personal && <Badge label="Personal" variant="warning" />}
              {badge}
            </HStack>
          }
        />
      ))}
    </List>
  );
}

/**
 * The custom fields the app keeps on the store's objects: which owners it adds fields
 * to, which are personal, and on an update which it removes.
 */
function CustomFields({ request }: { request: Request }) {
  const { added, kept, removed, retyped } = request.custom_fields;
  if (added.length + kept.length + removed.length + retyped.length === 0) return null;
  const owners = [...new Set(added.map((f) => OWNER_LABELS[f.owner_type] ?? f.owner_type))];
  return (
    <Card>
      <VStack gap={3}>
        <Text type="display-3" as="h2">
          Custom fields
        </Text>
        {owners.length > 0 && (
          <Text color="secondary">
            The app adds fields to {owners.join(", ").toLowerCase()}, in its own namespace (
            {request.app.key}).
          </Text>
        )}
        {retyped.length > 0 && (
          <Banner
            status="error"
            title="This version can't be approved"
            description={`It changes the type of ${retyped.map((f) => f.key).join(", ")}; a field's type can't change.`}
          />
        )}
        {added.length > 0 && (
          <FieldItems
            fields={added}
            badge={request.app.installed ? <Badge label="New" variant="info" /> : undefined}
          />
        )}
        {request.app.installed && kept.length > 0 && <FieldItems fields={kept} />}
        {removed.length > 0 && (
          <VStack gap={1}>
            <Text type="label">Removes fields</Text>
            <Text color="secondary">
              Approving deletes these fields and every value stored in them.
            </Text>
            <FieldItems fields={removed} badge={<Badge label="Removed" variant="error" />} />
          </VStack>
        )}
      </VStack>
    </Card>
  );
}

/** What approving changes compared with the app's current access. */
function Changes({ changes }: { changes: Request["changes"] }) {
  return (
    <Card>
      <VStack gap={3}>
        <Text type="display-3" as="h2">
          What changes
        </Text>
        <Text color="secondary">
          The app keeps its current access until you approve; denying leaves it as it is.
        </Text>
        <ChangeRow
          label="Added"
          scopes={changes.added}
          badge={<Badge label="+" variant="info" />}
        />
        <ChangeRow
          label="Removed"
          scopes={changes.removed}
          badge={<Badge label="−" variant="warning" />}
        />
        <ChangeRow label="Kept" scopes={changes.kept} badge={<Badge label="=" />} />
      </VStack>
    </Card>
  );
}
