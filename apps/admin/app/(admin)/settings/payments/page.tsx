import { Banner } from "@astryxdesign/core/Banner";
import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Suspense } from "react";

import { PageHeader } from "@/components/page-header";
import { CardSkeleton } from "@/components/skeletons";
import { channelNames, loadPaymentMethods, requireStaff, sessionClient } from "@/lib/hypha";
import { getT } from "@/lib/i18n";
import { settingsCrumb } from "@/lib/navigation";
import type { PaymentApp } from "@/lib/payments";

import { PaymentMethodsCard } from "./methods";

/** Settings → Payments: how shoppers can pay, in the order they see the methods at
 * checkout. The header renders at once; the methods stream in. */
export default async function PaymentsSettingsPage() {
  const t = await getT();
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={960}>
        <PageHeader
          back={settingsCrumb(t)}
          title={t("payments.settings_title")}
          description={t("payments.settings_description")}
        />
        <Suspense fallback={<CardSkeleton lines={5} />}>
          <PaymentMethods />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function PaymentMethods() {
  const client = await sessionClient();
  const [t, me, methods, markets, channelName, paymentApps] = await Promise.all([
    getT(),
    requireStaff(),
    loadPaymentMethods(),
    client.GET("/admin/markets"),
    channelNames(client),
    loadPaymentApps(client),
  ]);
  if (methods === null) {
    return (
      <Banner
        status="info"
        title={t("payments.settings_no_access_title")}
        description={t("payments.settings_no_access_description")}
      />
    );
  }
  return (
    <>
      {!methods.some((m) => m.enabled) && (
        <Banner
          status="warning"
          title={t("payments.settings_none_enabled_title")}
          description={t("payments.settings_none_enabled_description")}
        />
      )}
      <PaymentMethodsCard
        methods={methods}
        // Without markets:read or channels:read the pickers are empty and the
        // table shows IDs; the API still checks nothing it doesn't know.
        markets={(markets.data?.items ?? []).map((m) => ({ value: m.id, label: m.name }))}
        channels={[...channelName].map(([value, label]) => ({ value, label }))}
        paymentApps={paymentApps}
        canWrite={me.scopes.includes("payments:write")}
      />
    </>
  );
}

/**
 * What a method can name as its provider besides the manual kinds: the API's list
 * (`GET /admin/payment-providers`), the enabled spores built into it (Stripe) and the
 * installed, enabled apps answering `payments.start`. Without `payments:read` there are
 * none to offer; the manual kinds still are.
 */
async function loadPaymentApps(client: Awaited<ReturnType<typeof sessionClient>>) {
  const { data } = await client.GET("/admin/payment-providers");
  return (data?.items ?? [])
    .filter((p) => p.kind !== "manual")
    .map((p): PaymentApp => ({ key: p.key, name: p.name, isBuiltIn: p.kind === "spore" }));
}
