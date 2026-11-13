import { Banner } from "@astryxdesign/core/Banner";
import { Button } from "@astryxdesign/core/Button";
import { Card } from "@astryxdesign/core/Card";
import { ClickableCard } from "@astryxdesign/core/ClickableCard";
import { Grid } from "@astryxdesign/core/Grid";
import { HStack, VStack } from "@astryxdesign/core/Layout";
import { Link } from "@astryxdesign/core/Link";
import { Section } from "@astryxdesign/core/Section";
import { Skeleton } from "@astryxdesign/core/Skeleton";
import { Text } from "@astryxdesign/core/Text";
import { CheckCircleIcon as CheckCircleOutline } from "@heroicons/react/24/outline";
import { CheckCircleIcon } from "@heroicons/react/24/solid";
import { withEnglish } from "@hyphacommerce/i18n";
import type { AdminPaths, AdminSchemas } from "@hyphacommerce/sdk/admin";
import { Suspense } from "react";

import { DateTime } from "@/components/date-time";
import { PageHeader } from "@/components/page-header";
import { startOfDay, nextDay } from "@/components/resource-table/server";
import { CardSkeleton } from "@/components/skeletons";
import { CancelledBadge, FulfillmentBadge, PaymentBadge } from "@/components/status";
import { type Format, getFormat } from "@/lib/get-format";
import { loadSetup, loadStore, loadTimeZone, requireStaff, sessionClient } from "@/lib/hypha";
import { type T, getT } from "@/lib/i18n";
import { LOW_STOCK } from "@/lib/inventory";
import { hasScope } from "@/lib/navigation";

import { type SetupStep, setupSteps } from "./setup-checklist";

type Client = Awaited<ReturnType<typeof sessionClient>>;
/** The orders list's filters. */
type OrderQuery = NonNullable<AdminPaths["/admin/orders"]["get"]["parameters"]["query"]>;

/** How many orders a tile counts before it says "100+": the list's largest page. */
const COUNT_LIMIT = 100;
/** How many orders the recent list shows. */
const RECENT = 5;

/**
 * The signed-in home: a greeting, what needs doing (the setup checklist until it's
 * finished, agents' proposals to review and stock running low, today's orders and sales,
 * orders to fulfil and payments to collect) and the latest orders. The greeting and the banners render at once; each card streams in.
 */
export default async function HomePage() {
  const [me, store, setup, timeZone, t] = await Promise.all([
    requireStaff(),
    loadStore(),
    loadSetup(),
    loadTimeZone(),
    getT(),
  ]);
  const showChecklist = setup !== null && !setup.isFinished;
  const canSeeOrders = hasScope(me.scopes, "orders:read");
  const canApprove = hasScope(me.scopes, "agents:read");
  const canSeeStock = hasScope(me.scopes, "inventory:read");
  // A server component renders once per request, so reading the clock here is fine.
  // oxlint-disable-next-line react/purity
  const now = new Date();

  return (
    <Section padding={6}>
      <VStack gap={6} maxWidth={1280}>
        <PageHeader
          title={t(`home.greeting_${dayPart(now, timeZone)}`, { name: me.user.name })}
          description={[longDate(now, timeZone, t.locale), store?.name].filter(Boolean).join(" · ")}
          actions={
            <>
              {canSeeOrders && (
                <Button label={t("home.view_orders")} variant="secondary" href="/orders" />
              )}
              {hasScope(me.scopes, "products:write") && (
                <Button label={t("home.add_product")} variant="primary" href="/products/new" />
              )}
            </>
          }
        />

        {!me.mfa_enabled && (
          <Banner
            status="warning"
            title={t("home.two_factor_title")}
            description={t("home.two_factor_description")}
            endContent={
              <Button
                label={t("home.two_factor_action")}
                variant="secondary"
                href="/account#two-factor"
              />
            }
          />
        )}

        {/* Only the setup's state needs the store's data; nothing else waits for it. */}
        {(showChecklist || hasPaymentsRead(me)) && (
          <Suspense fallback={showChecklist ? <CardSkeleton lines={4} /> : null}>
            <SetupState me={me} showChecklist={showChecklist} />
          </Suspense>
        )}

        {(canApprove || canSeeStock) && (
          <Suspense fallback={null}>
            <Attention canApprove={canApprove} canSeeStock={canSeeStock} />
          </Suspense>
        )}

        {canSeeOrders && (
          <>
            <Suspense fallback={<TilesSkeleton />}>
              <Tiles now={now} timeZone={timeZone} />
            </Suspense>
            <Suspense fallback={<CardSkeleton lines={RECENT} />}>
              <RecentOrders />
            </Suspense>
          </>
        )}
      </VStack>
    </Section>
  );
}

function hasPaymentsRead(me: AdminSchemas["MeResponse"]): boolean {
  return hasScope(me.scopes, "payments:read");
}

/**
 * Which greeting the hour calls for, by the store's clock, so a team abroad greets the
 * store's day. The hour is computed, never shown: its locale is fixed.
 */
function dayPart(now: Date, timeZone: string): "morning" | "afternoon" | "evening" {
  const hour = Number(
    new Intl.DateTimeFormat("en-US", { hour: "numeric", hourCycle: "h23", timeZone }).format(now),
  );
  return hour < 12 ? "morning" : hour < 18 ? "afternoon" : "evening";
}

/**
 * "Thursday 22 October", in the store's time zone, as `locale` writes it. `Format` has no
 * date with its weekday, so this one asks `Intl` itself, with the translator's locale.
 */
function longDate(now: Date, timeZone: string, locale: string): string {
  return new Intl.DateTimeFormat(withEnglish(locale), {
    weekday: "long",
    day: "numeric",
    month: "long",
    timeZone,
  }).format(now);
}

/** Today as YYYY-MM-DD in the store's time zone: computed, never shown, so its locale is fixed. */
function today(now: Date, timeZone: string): string {
  return new Intl.DateTimeFormat("en-CA", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    timeZone,
  }).format(now);
}

// ---------- setup ----------

/** The checklist while setup is unfinished, and the warning while checkout is refused. */
async function SetupState({
  me,
  showChecklist,
}: {
  me: AdminSchemas["MeResponse"];
  showChecklist: boolean;
}) {
  const t = await getT();
  const steps = await setupSteps(t);
  return (
    <>
      {/* Only for staff who can see methods: without payments:read the checklist's
          step reads as not done whatever the store has. */}
      {hasPaymentsRead(me) && steps.some((step) => step.key === "payment" && !step.isDone) && (
        <Banner
          status="warning"
          title={t("home.no_payment_title")}
          description={t("home.no_payment_description")}
          endContent={
            <Button
              label={t("home.no_payment_action")}
              variant="secondary"
              href="/settings/payments"
            />
          }
        />
      )}
      {showChecklist && <SetupChecklist steps={steps} t={t} />}
    </>
  );
}

/** What a storefront needs before it can sell, ticked as the data shows it done. */
function SetupChecklist({ steps, t }: { steps: SetupStep[]; t: T }) {
  const done = steps.filter((step) => step.isDone).length;
  return (
    <Card>
      <VStack gap={4}>
        <HStack gap={3} hAlign="between" vAlign="start" wrap="wrap">
          <VStack gap={1}>
            <Text type="display-3" as="h2">
              {t("home.checklist_title")}
            </Text>
            <Text type="supporting" color="secondary">
              {t("home.checklist_progress", { done, total: steps.length })}
            </Text>
          </VStack>
          <Button label={t("home.checklist_continue")} variant="primary" href="/setup" />
        </HStack>
        <VStack gap={3}>
          {steps.map((step) => (
            <HStack key={step.key} gap={3} vAlign="start">
              <Text color={step.isDone ? undefined : "secondary"}>
                {step.isDone ? (
                  <CheckCircleIcon width={20} aria-label={t("common.done")} />
                ) : (
                  <CheckCircleOutline width={20} aria-label={t("home.checklist_not_done")} />
                )}
              </Text>
              <VStack gap={0}>
                {step.isDone ? (
                  <Text color="secondary">{step.label}</Text>
                ) : (
                  <Link href={step.href} weight="medium">
                    {step.label}
                  </Link>
                )}
                <Text type="supporting" color="secondary">
                  {step.description}
                </Text>
              </VStack>
            </HStack>
          ))}
        </VStack>
      </VStack>
    </Card>
  );
}

// ---------- today's numbers ----------

/** A tile: its value is one figure, or one per currency (wrapping between them). */
type Tile = { label: string; value: string | string[]; hint: string; href: string };

/**
 * Today's orders and sales, the orders still to ship and the payments still to collect,
 * each a link to the orders list filtered the same way. A count stops at the list's
 * largest page ("100+"): the tiles say what needs attention, not the books.
 */
async function Tiles({ now, timeZone }: { now: Date; timeZone: string }) {
  const [client, t, format] = await Promise.all([sessionClient(), getT(), getFormat()]);
  const day = today(now, timeZone);
  const placed = {
    placed_from: startOfDay(day, timeZone),
    placed_before: startOfDay(nextDay(day), timeZone),
  };
  const toFulfil: OrderQuery = {
    fulfillment_status: ["unfulfilled", "partially_fulfilled"],
    cancelled: false,
  };
  const toCollect: OrderQuery = { financial_status: ["pending"], cancelled: false };
  const [todays, report, unfulfilled, unpaid] = await Promise.all([
    list(client, placed),
    client.GET("/admin/reports/sales", {
      params: { query: { from: placed.placed_from, to: placed.placed_before, top: 1 } },
    }),
    list(client, toFulfil),
    list(client, toCollect),
  ]);

  const tiles: Tile[] = [
    {
      label: t("home.tile_orders_today"),
      value: count(t, todays),
      hint: t("home.tile_orders_today_hint"),
      href: `/orders?placed=${day}..${day}`,
    },
    {
      label: t("home.tile_sales_today"),
      value: sales(t, format, report.data?.total.sales),
      hint: t("home.tile_sales_today_hint"),
      href: `/orders?placed=${day}..${day}`,
    },
    {
      label: t("home.tile_to_fulfil"),
      value: count(t, unfulfilled),
      hint: t("home.tile_to_fulfil_hint"),
      href: "/orders?fulfillment_status=unfulfilled&fulfillment_status=partially_fulfilled&cancelled=false",
    },
    {
      label: t("home.tile_awaiting_payment"),
      value: count(t, unpaid),
      hint: t("home.tile_awaiting_payment_hint"),
      href: "/orders?financial_status=pending&cancelled=false",
    },
  ];
  return <TileGrid tiles={tiles} />;
}

function TileGrid({ tiles }: { tiles: Tile[] }) {
  return (
    <Grid columns={{ minWidth: 200, repeat: "fill" }} gap={3}>
      {tiles.map((tile) => (
        <ClickableCard key={tile.label} label={tile.label} href={tile.href} padding={4}>
          <VStack gap={1}>
            <Text type="supporting" color="secondary">
              {tile.label}
            </Text>
            <Text type="display-2" hasTabularNumbers>
              {typeof tile.value === "string" ? (
                tile.value
              ) : (
                <span className="tile-amounts">
                  {tile.value.map((amount) => (
                    <span key={amount}>{amount}</span>
                  ))}
                </span>
              )}
            </Text>
            <Text type="supporting" color="secondary">
              {tile.hint}
            </Text>
          </VStack>
        </ClickableCard>
      ))}
    </Grid>
  );
}

/**
 * What waits on staff beyond orders: agents' proposals they can approve (only when there
 * are some) and variants running low on stock, each a link to the list that shows them.
 */
async function Attention({
  canApprove,
  canSeeStock,
}: {
  canApprove: boolean;
  canSeeStock: boolean;
}) {
  const [client, t] = await Promise.all([sessionClient(), getT()]);
  const [proposals, low] = await Promise.all([
    canApprove
      ? client.GET("/admin/agent-proposals", {
          params: { query: { status: "pending", approvable: true, limit: COUNT_LIMIT } },
        })
      : undefined,
    canSeeStock
      ? client.GET("/admin/inventory-items", {
          params: { query: { below: LOW_STOCK, limit: COUNT_LIMIT } },
        })
      : undefined,
  ]);
  const tiles: Tile[] = [];
  const waiting = proposals?.data;
  if (waiting && waiting.items.length > 0) {
    tiles.push({
      label: t("home.tile_proposals"),
      value: counted(t, waiting.items.length, waiting.next_cursor != null),
      hint: t("home.tile_proposals_hint"),
      href: "/agents/proposals",
    });
  }
  const running = low?.data;
  if (running) {
    tiles.push({
      label: t("home.tile_low_stock"),
      value: counted(t, running.items.length, running.next_cursor != null),
      hint: t("home.tile_low_stock_hint", { count: LOW_STOCK }),
      href: `/inventory?below=${LOW_STOCK}`,
    });
  }
  if (tiles.length === 0) return null;
  return <TileGrid tiles={tiles} />;
}

type Listed = { items: AdminSchemas["OrderSummaryResponse"][]; more: boolean } | null;

/** Up to a page of orders matching `query`; `null` when the API refused the read. */
async function list(client: Client, query: OrderQuery): Promise<Listed> {
  const { data } = await client.GET("/admin/orders", {
    params: { query: { ...query, limit: COUNT_LIMIT } },
  });
  return data ? { items: data.items, more: data.next_cursor != null } : null;
}

/** "12", or "100+" when there are more than were read. */
function counted(t: T, shown: number, more: boolean): string {
  return more ? t("home.count_or_more", { count: shown }) : t("home.count", { count: shown });
}

function count(t: T, listed: Listed): string {
  if (!listed) return t("common.empty_value");
  return counted(t, listed.items.length, listed.more);
}

/** Today's sales, one amount per currency; "—" with none. */
function sales(
  t: T,
  format: Format,
  rows: AdminSchemas["SalesRowResponse"][] | undefined,
): string | string[] {
  if (!rows || rows.length === 0) return t("common.empty_value");
  return rows.map((row) => format.money(row.total));
}

function TilesSkeleton() {
  return (
    <Grid columns={{ minWidth: 200, repeat: "fill" }} gap={3}>
      {Array.from({ length: 4 }, (_, i) => (
        <Card key={i} padding={4}>
          <VStack gap={2}>
            <Skeleton width={90} height={14} />
            <Skeleton width={120} height={32} />
            <Skeleton width={140} height={14} />
          </VStack>
        </Card>
      ))}
    </Grid>
  );
}

// ---------- recent orders ----------

/** The latest orders, newest first, as on a customer's page. */
async function RecentOrders() {
  const [client, format, t] = await Promise.all([sessionClient(), getFormat(), getT()]);
  const { data } = await client.GET("/admin/orders", { params: { query: { limit: RECENT } } });
  const orders = data?.items ?? [];
  return (
    <Card>
      <VStack gap={3}>
        <HStack gap={3} hAlign="between" vAlign="center">
          <Text type="display-3" as="h2">
            {t("home.recent_title")}
          </Text>
          <Link href="/orders">{t("home.recent_view_all")}</Link>
        </HStack>
        {orders.length === 0 ? (
          <Text color="secondary">{t("home.recent_empty")}</Text>
        ) : (
          <VStack gap={0}>
            {orders.map((o, i) => (
              <div key={o.id} className={i > 0 ? "list-row list-row-divided" : "list-row"}>
                <Link href={`/orders/${o.id}`} weight="medium">
                  #{o.number}
                </Link>
                <Text color="secondary">
                  <DateTime value={o.placed_at} short />
                </Text>
                <HStack gap={1} wrap="wrap">
                  {o.cancelled_at ? (
                    <CancelledBadge t={t} />
                  ) : (
                    <>
                      <PaymentBadge status={o.financial_status} t={t} />
                      <FulfillmentBadge status={o.fulfillment_status} t={t} />
                    </>
                  )}
                </HStack>
                <Text hasTabularNumbers>{format.money(o.total)}</Text>
              </div>
            ))}
          </VStack>
        )}
      </VStack>
    </Card>
  );
}
