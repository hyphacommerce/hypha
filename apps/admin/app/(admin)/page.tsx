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
import type { AdminPaths, AdminSchemas } from "@hypha/sdk/admin";
import { Suspense } from "react";

import { DateTime } from "@/components/date-time";
import { PageHeader } from "@/components/page-header";
import { startOfDay, nextDay } from "@/components/resource-table/server";
import { CardSkeleton } from "@/components/skeletons";
import { CancelledBadge, FulfillmentBadge, PaymentBadge } from "@/components/status";
import { type Currencies, formatMoney } from "@/lib/format";
import {
  loadCurrencies,
  loadSetup,
  loadStore,
  loadTimeZone,
  requireStaff,
  sessionClient,
} from "@/lib/hypha";
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
 * finished, today's orders and sales, orders to fulfil and payments to collect) and the
 * latest orders. The greeting and the banners render at once; each card streams in.
 */
export default async function HomePage() {
  const [me, store, setup, timeZone] = await Promise.all([
    requireStaff(),
    loadStore(),
    loadSetup(),
    loadTimeZone(),
  ]);
  const showChecklist = setup !== null && !setup.isFinished;
  const canSeeOrders = hasScope(me.scopes, "orders:read");
  // A server component renders once per request, so reading the clock here is fine.
  // oxlint-disable-next-line react/purity
  const now = new Date();

  return (
    <Section padding={6}>
      <VStack gap={6} maxWidth={1280}>
        <PageHeader
          title={`${greeting(now, timeZone)}, ${me.user.name}`}
          description={[longDate(now, timeZone), store?.name].filter(Boolean).join(" · ")}
          actions={
            <>
              {canSeeOrders && <Button label="View orders" variant="secondary" href="/orders" />}
              {hasScope(me.scopes, "products:write") && (
                <Button label="Add product" variant="primary" href="/products/new" />
              )}
            </>
          }
        />

        {!me.mfa_enabled && (
          <Banner
            status="warning"
            title="Two-factor authentication is off"
            description="Turn it on to protect your account; it takes a minute with any authenticator app."
            endContent={<Button label="Set it up" variant="secondary" href="/account#two-factor" />}
          />
        )}

        {/* Only the setup's state needs the store's data; nothing else waits for it. */}
        {(showChecklist || hasPaymentsRead(me)) && (
          <Suspense fallback={showChecklist ? <CardSkeleton lines={4} /> : null}>
            <SetupState me={me} showChecklist={showChecklist} />
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

/** "Good morning" by the store's clock, so a team abroad greets the store's day. */
function greeting(now: Date, timeZone: string): string {
  const hour = Number(
    new Intl.DateTimeFormat("en-US", { hour: "numeric", hourCycle: "h23", timeZone }).format(now),
  );
  return hour < 12 ? "Good morning" : hour < 18 ? "Good afternoon" : "Good evening";
}

/** "Thursday 22 October", in the store's time zone. */
function longDate(now: Date, timeZone: string): string {
  return new Intl.DateTimeFormat("en-GB", {
    weekday: "long",
    day: "numeric",
    month: "long",
    timeZone,
  }).format(now);
}

/** Today as YYYY-MM-DD in the store's time zone. */
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
  const steps = await setupSteps();
  return (
    <>
      {/* Only for staff who can see methods: without payments:read the checklist's
          step reads as not done whatever the store has. */}
      {hasPaymentsRead(me) && steps.some((step) => step.key === "payment" && !step.isDone) && (
        <Banner
          status="warning"
          title="Checkout is refused until a payment method is enabled"
          description="Shoppers can't place orders with something to pay."
          endContent={
            <Button label="Set up payments" variant="secondary" href="/settings/payments" />
          }
        />
      )}
      {showChecklist && <SetupChecklist steps={steps} />}
    </>
  );
}

/** What a storefront needs before it can sell, ticked as the data shows it done. */
function SetupChecklist({ steps }: { steps: SetupStep[] }) {
  const done = steps.filter((step) => step.isDone).length;
  return (
    <Card>
      <VStack gap={4}>
        <HStack gap={3} hAlign="between" vAlign="start" wrap="wrap">
          <VStack gap={1}>
            <Text type="display-3" as="h2">
              Get ready to sell
            </Text>
            <Text type="supporting" color="secondary">
              {done} of {steps.length} done. Each step opens the screen it's done on.
            </Text>
          </VStack>
          <Button label="Continue the setup guide" variant="primary" href="/setup" />
        </HStack>
        <VStack gap={3}>
          {steps.map((step) => (
            <HStack key={step.label} gap={3} vAlign="start">
              <Text color={step.isDone ? undefined : "secondary"}>
                {step.isDone ? (
                  <CheckCircleIcon width={20} aria-label="Done" />
                ) : (
                  <CheckCircleOutline width={20} aria-label="Not done yet" />
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

type Tile = { label: string; value: string; hint: string; href: string };

/**
 * Today's orders and sales, the orders still to ship and the payments still to collect,
 * each a link to the orders list filtered the same way. A count stops at the list's
 * largest page ("100+"): the tiles say what needs attention, not the books.
 */
async function Tiles({ now, timeZone }: { now: Date; timeZone: string }) {
  const [client, currencies] = await Promise.all([sessionClient(), loadCurrencies()]);
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
  const [todays, unfulfilled, unpaid] = await Promise.all([
    list(client, placed),
    list(client, toFulfil),
    list(client, toCollect),
  ]);

  const tiles: Tile[] = [
    {
      label: "Orders today",
      value: count(todays),
      hint: "Placed since midnight",
      href: `/orders?placed=${day}..${day}`,
    },
    {
      label: "Sales today",
      value: sales(todays, currencies),
      hint: "Order totals, before refunds",
      href: `/orders?placed=${day}..${day}`,
    },
    {
      label: "To fulfil",
      value: count(unfulfilled),
      hint: "Not yet shipped in full",
      href: "/orders?fulfillment_status=unfulfilled&fulfillment_status=partially_fulfilled&cancelled=false",
    },
    {
      label: "Awaiting payment",
      value: count(unpaid),
      hint: "Manual methods to mark paid",
      href: "/orders?financial_status=pending&cancelled=false",
    },
  ];
  return (
    <Grid columns={{ minWidth: 200, repeat: "fill" }} gap={3}>
      {tiles.map((tile) => (
        <ClickableCard key={tile.label} label={tile.label} href={tile.href} padding={4}>
          <VStack gap={1}>
            <Text type="supporting" color="secondary">
              {tile.label}
            </Text>
            <Text type="display-2" hasTabularNumbers>
              {tile.value}
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

type Listed = { items: AdminSchemas["OrderSummaryResponse"][]; more: boolean } | null;

/** Up to a page of orders matching `query`; `null` when the API refused the read. */
async function list(client: Client, query: OrderQuery): Promise<Listed> {
  const { data } = await client.GET("/admin/orders", {
    params: { query: { ...query, limit: COUNT_LIMIT } },
  });
  return data ? { items: data.items, more: data.next_cursor != null } : null;
}

function count(listed: Listed): string {
  if (!listed) return "—";
  return listed.more ? `${listed.items.length}+` : String(listed.items.length);
}

/** The orders' totals summed per currency ("€1,240.00 · $80.00"); "—" with none. */
function sales(listed: Listed, currencies: Currencies): string {
  if (!listed || listed.items.length === 0) return "—";
  const byCurrency = new Map<string, number>();
  for (const order of listed.items) {
    if (order.cancelled_at) continue;
    const { amount, currency } = order.total;
    byCurrency.set(currency, (byCurrency.get(currency) ?? 0) + amount);
  }
  if (byCurrency.size === 0) return "—";
  return [...byCurrency]
    .map(([currency, amount]) => formatMoney({ amount, currency }, currencies))
    .join(" · ");
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
  const [client, currencies] = await Promise.all([sessionClient(), loadCurrencies()]);
  const { data } = await client.GET("/admin/orders", { params: { query: { limit: RECENT } } });
  const orders = data?.items ?? [];
  return (
    <Card>
      <VStack gap={3}>
        <HStack gap={3} hAlign="between" vAlign="center">
          <Text type="display-3" as="h2">
            Recent orders
          </Text>
          <Link href="/orders">View all</Link>
        </HStack>
        {orders.length === 0 ? (
          <Text color="secondary">No orders yet. They show up here as shoppers place them.</Text>
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
                    <CancelledBadge />
                  ) : (
                    <>
                      <PaymentBadge status={o.financial_status} />
                      <FulfillmentBadge status={o.fulfillment_status} />
                    </>
                  )}
                </HStack>
                <Text hasTabularNumbers>{formatMoney(o.total, currencies)}</Text>
              </div>
            ))}
          </VStack>
        )}
      </VStack>
    </Card>
  );
}
