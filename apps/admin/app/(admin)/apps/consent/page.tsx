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
import type { AdminSchemas } from "@hyphacommerce/sdk/admin";
import { type ReactNode, Suspense } from "react";

import { DateTime } from "@/components/date-time";
import { PageHeader } from "@/components/page-header";
import { FormSkeleton } from "@/components/skeletons";
import { areaLabel, personalDataLabel, scopeLabel } from "@/lib/apps";
import { type Format, getFormat } from "@/lib/get-format";
import { isMissingScope, requireStaff, sessionClient } from "@/lib/hypha";
import { getT } from "@/lib/i18n";
import { labelOr } from "@/lib/keys";
import type { T } from "@/lib/messages";
import { appsCrumb } from "@/lib/navigation";

import { ConsentButtons } from "./consent-buttons";

type Request = AdminSchemas["ConsentRequestResponse"];

/**
 * The consent page: where Hypha's `/oauth/authorize` sends staff (`?request=oareq_…`),
 * and where an uploaded private app's manifest waits. It shows who the app is, what it
 * asks for in plain words, the personal data it would see with the app's reasons, and
 * what changes compared with its current access; staff who may approve answer, and the
 * browser goes where the API says. Signed out, the login comes first and returns here
 * (`proxy.ts`). A link without a request is answered at once; with one, the way back
 * renders at once and the request streams in behind a skeleton of the page.
 */
export default async function ConsentPage({
  searchParams,
}: {
  searchParams: Promise<Record<string, string | string[] | undefined>>;
}) {
  const [{ request: id }, t] = await Promise.all([searchParams, getT()]);
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={760}>
        {typeof id !== "string" || !id.startsWith("oareq_") ? (
          <Problem
            t={t}
            title={t("apps.consent_no_request_title")}
            description={t("apps.consent_no_request_description")}
          />
        ) : (
          <Suspense
            fallback={
              <>
                <PageHeader back={appsCrumb(t)} />
                <FormSkeleton cards={2} />
              </>
            }
          >
            <ConsentRequest id={id} />
          </Suspense>
        )}
      </VStack>
    </Section>
  );
}

async function ConsentRequest({ id }: { id: string }) {
  const [client, me, t, format] = await Promise.all([
    sessionClient(),
    requireStaff(),
    getT(),
    getFormat(),
  ]);
  const result = await client.GET("/admin/oauth/requests/{id}", { params: { path: { id } } });
  if (isMissingScope(result)) {
    return (
      <Problem
        t={t}
        title={t("apps.consent_cannot_answer_title")}
        description={t("apps.consent_cannot_answer_description", { scope: "apps:read" })}
      />
    );
  }
  if (!result.data) {
    return (
      <Problem
        t={t}
        title={t("apps.consent_missing_title")}
        description={t("apps.consent_no_request_description")}
      />
    );
  }
  const request = result.data;
  const missing = request.scopes.filter((s) => !me.scopes.includes(s));
  const canWrite = me.scopes.includes("apps:write");
  const blocked = !canWrite
    ? t("apps.consent_blocked_no_write", { scope: "apps:write" })
    : missing.length > 0
      ? t("apps.consent_blocked_missing_scopes", { scopes: format.list(missing, "unit") })
      : undefined;
  const redirectHost = hostOf(request.redirect_uri);
  const waitsUntil = <DateTime key="time" value={request.expires_at} short />;

  return (
    <>
      <PageHeader
        title={
          request.app.installed
            ? t("apps.consent_title_more", { name: request.app.name })
            : t("apps.consent_title_install", { name: request.app.name })
        }
        back={appsCrumb(t)}
        badges={<StatusBadge t={t} status={request.status} />}
        description={
          request.app.private
            ? t("apps.state_private_description")
            : t.rich("apps.consent_asked", {
                time: <DateTime key="time" value={request.created_at} short />,
              })
        }
      />

      <AppIdentity t={t} request={request} />

      {request.status === "expired" && (
        <Banner
          status="warning"
          title={t("apps.consent_expired_title")}
          description={t("apps.consent_expired_description")}
        />
      )}
      {(request.status === "approved" || request.status === "denied") && (
        <Banner
          status="info"
          title={
            request.status === "approved"
              ? t("apps.consent_already_approved")
              : t("apps.consent_already_denied")
          }
          description={t("apps.consent_answered_once")}
          endContent={<Link href={`/apps/${request.app.key}`}>{t("apps.consent_open_app")}</Link>}
        />
      )}

      {request.app.installed && <Changes t={t} changes={request.changes} />}

      <Card>
        <VStack gap={4}>
          <Text type="display-3" as="h2">
            {t("apps.consent_may_do_title")}
          </Text>
          {request.groups.length === 0 ? (
            <Text color="secondary">{t("apps.consent_may_do_nothing")}</Text>
          ) : (
            request.groups.map((group) => (
              <VStack key={group.area} gap={1}>
                <Text type="label">{areaLabel(t, group.area)}</Text>
                <List>
                  {group.scopes.map((scope) => (
                    <ListItem
                      key={scope}
                      label={scopeLabel(t, scope)}
                      description={scope}
                      endContent={
                        request.app.installed && request.changes.added.includes(scope) ? (
                          <Badge label={t("apps.consent_new")} variant="info" />
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
            {t("apps.detail_personal_title")}
          </Text>
          {request.personal_data.length === 0 ? (
            <Text color="secondary">{t("apps.consent_personal_none")}</Text>
          ) : (
            <>
              <Text color="secondary">{t("apps.consent_personal_intro")}</Text>
              <List hasDividers>
                {request.personal_data.map((item) => (
                  <ListItem
                    key={item.scope}
                    label={personalDataLabel(t, item.scope)}
                    description={t("apps.consent_reason", { reason: item.reason })}
                    endContent={<Badge label={item.scope} variant="warning" />}
                  />
                ))}
              </List>
              <Text type="supporting" color="secondary">
                {t.rich("apps.consent_privacy_hint", {
                  link: (chunks, key) => (
                    <Link key={key} href={request.app.privacy_url} isExternalLink>
                      {chunks}
                    </Link>
                  ),
                })}
              </Text>
            </>
          )}
        </VStack>
      </Card>

      <CustomFields t={t} format={format} request={request} />

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
            <Banner
              status="warning"
              title={t("apps.consent_cannot_approve_title")}
              description={blocked}
            />
          ) : (
            <Text color="secondary">
              {request.app.private
                ? t.rich("apps.consent_intro_private", { time: waitsUntil })
                : redirectHost
                  ? t.rich("apps.consent_intro_redirect", { host: redirectHost, time: waitsUntil })
                  : t.rich("apps.consent_intro_redirect_app", { time: waitsUntil })}
            </Text>
          )
        }
      />
    </>
  );
}

/** A URL's host, or `null` for one that isn't a URL (the page then says "the app"). */
function hostOf(url: string): string | null {
  try {
    return new URL(url).host;
  } catch {
    return null;
  }
}

/** Why there's nothing to answer, in place of the page's body. */
function Problem({ t, title, description }: { t: T; title: string; description: string }) {
  return (
    <EmptyState
      icon={<ExclamationTriangleIcon width={32} />}
      title={title}
      description={description}
      actions={<Link href="/apps">{t("apps.consent_back_to_apps")}</Link>}
    />
  );
}

function StatusBadge({ t, status }: { t: T; status: Request["status"] }) {
  switch (status) {
    case "pending":
      return <Badge label={t("apps.consent_status_pending")} variant="info" />;
    case "approved":
      return <Badge label={t("apps.consent_status_approved")} variant="success" />;
    case "denied":
      return <Badge label={t("apps.consent_status_denied")} variant="error" />;
    case "expired":
      return <Badge label={t("apps.consent_status_expired")} variant="warning" />;
  }
}

/** Who the app is: its icon, name, publisher and site. */
function AppIdentity({ t, request }: { t: T; request: Request }) {
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
            {app.publisher
              ? `${t("apps.consent_by_publisher", { publisher: app.publisher })} · `
              : ""}
            <Link href={app.app_url} isExternalLink>
              {hostOf(app.app_url) ?? t("apps.consent_the_app")}
            </Link>
            {" · "}
            <Link href={app.privacy_url} isExternalLink>
              {t("apps.consent_privacy_policy")}
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
  t,
  label,
  scopes,
  badge,
}: {
  t: T;
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
              <Text>{scopeLabel(t, scope)}</Text>
            </HStack>
          ))}
        </HStack>
      </VStack>
    )
  );
}

/** What a field belongs to, as a heading ("Products"); an unknown kind shows as it is. */
function ownerLabel(t: T, owner: string): string {
  switch (owner) {
    case "product":
      return t("common.products");
    case "variant":
      return t("common.variants");
    case "category":
      return t("common.categories");
    case "customer":
      return t("common.customers");
    case "order":
      return t("common.orders");
    default:
      return owner;
  }
}

/** The same within a sentence ("fields to products, variants"). */
function ownerInSentence(t: T, owner: string): string {
  const key = `apps.consent_fields_owner_${owner}`;
  return labelOr(t, key, owner);
}

type Field = Request["custom_fields"]["added"][number];

/** One line per field: its name, where it lives and whether it's about a person. */
function FieldItems({ t, fields, badge }: { t: T; fields: Field[]; badge?: ReactNode }) {
  return (
    <List>
      {fields.map((field) => (
        <ListItem
          key={`${field.owner_type}.${field.key}`}
          label={field.name}
          description={`${ownerLabel(t, field.owner_type)} · ${field.key} · ${field.type}`}
          endContent={
            <HStack gap={1}>
              {field.personal && (
                <Badge label={t("apps.consent_field_personal")} variant="warning" />
              )}
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
function CustomFields({ t, format, request }: { t: T; format: Format; request: Request }) {
  const { added, kept, removed, retyped } = request.custom_fields;
  if (added.length + kept.length + removed.length + retyped.length === 0) return null;
  const owners = [...new Set(added.map((f) => ownerInSentence(t, f.owner_type)))];
  return (
    <Card>
      <VStack gap={3}>
        <Text type="display-3" as="h2">
          {t("apps.consent_fields_title")}
        </Text>
        {owners.length > 0 && (
          <Text color="secondary">
            {t("apps.consent_fields_added_to", {
              owners: format.list(owners, "unit"),
              namespace: request.app.key,
            })}
          </Text>
        )}
        {retyped.length > 0 && (
          <Banner
            status="error"
            title={t("apps.consent_fields_retyped_title")}
            description={t("apps.consent_fields_retyped_description", {
              fields: format.list(
                retyped.map((f) => f.key),
                "unit",
              ),
            })}
          />
        )}
        {added.length > 0 && (
          <FieldItems
            t={t}
            fields={added}
            badge={
              request.app.installed ? (
                <Badge label={t("apps.consent_new")} variant="info" />
              ) : undefined
            }
          />
        )}
        {request.app.installed && kept.length > 0 && <FieldItems t={t} fields={kept} />}
        {removed.length > 0 && (
          <VStack gap={1}>
            <Text type="label">{t("apps.consent_fields_removes")}</Text>
            <Text color="secondary">{t("apps.consent_fields_removes_description")}</Text>
            <FieldItems
              t={t}
              fields={removed}
              badge={<Badge label={t("apps.consent_fields_removed")} variant="error" />}
            />
          </VStack>
        )}
      </VStack>
    </Card>
  );
}

/** What approving changes compared with the app's current access. */
function Changes({ t, changes }: { t: T; changes: Request["changes"] }) {
  return (
    <Card>
      <VStack gap={3}>
        <Text type="display-3" as="h2">
          {t("apps.consent_changes_title")}
        </Text>
        <Text color="secondary">{t("apps.consent_changes_description")}</Text>
        <ChangeRow
          t={t}
          label={t("apps.consent_changes_added")}
          scopes={changes.added}
          badge={<Badge label="+" variant="info" />}
        />
        <ChangeRow
          t={t}
          label={t("apps.consent_changes_removed")}
          scopes={changes.removed}
          badge={<Badge label="−" variant="warning" />}
        />
        <ChangeRow
          t={t}
          label={t("apps.consent_changes_kept")}
          scopes={changes.kept}
          badge={<Badge label="=" />}
        />
      </VStack>
    </Card>
  );
}
