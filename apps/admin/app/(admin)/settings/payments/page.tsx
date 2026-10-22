import { Banner } from "@astryxdesign/core/Banner";
import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";

import { PageHeader } from "@/components/page-header";
import { SETTINGS_CRUMB } from "@/lib/navigation";
import { channelNames, loadPaymentMethods, requireStaff, sessionClient } from "@/lib/hypha";
import type { PaymentApp } from "@/lib/payments";

import { PaymentMethodsCard } from "./methods";

/** Settings → Payments: how shoppers can pay, in the order they see the methods at
 * checkout. */
export default async function PaymentsSettingsPage() {
  const client = await sessionClient();
  const [me, methods, markets, channelName, paymentApps] = await Promise.all([
    requireStaff(),
    loadPaymentMethods(),
    client.GET("/admin/markets"),
    channelNames(client),
    loadPaymentApps(client),
  ]);

  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={960}>
        <PageHeader
          back={SETTINGS_CRUMB}
          title="Payments"
          description="How shoppers pay at checkout. With manual methods, like cash on delivery or a bank transfer, you collect the money yourself and mark the order paid."
        />
        {methods === null ? (
          <Banner
            status="info"
            title="Your role can't see payment methods"
            description="Ask the store owner for the payments:read scope."
          />
        ) : (
          <>
            {!methods.some((m) => m.enabled) && (
              <Banner
                status="warning"
                title="Checkout is refused until a payment method is enabled"
                description="Carts with something to pay can't be checked out. Add a method below, or turn one on."
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
        )}
      </VStack>
    </Section>
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
