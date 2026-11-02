import { Banner } from "@astryxdesign/core/Banner";
import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Suspense } from "react";

import { PageHeader } from "@/components/page-header";
import { CardSkeleton } from "@/components/skeletons";
import { isMissingScope, loaded, requireStaff, sessionClient } from "@/lib/hypha";
import { SETTINGS_CRUMB } from "@/lib/navigation";

import { AppSettingCard } from "./app-setting-card";
import { loadAnsweringApps, loadRuleOptions } from "./data";
import { RulesCard } from "./rules-card";
import { ValidationAppsCard } from "./validation-apps-card";

/**
 * Settings → Checkout: the rules a cart must meet to be checked out, in the order
 * they're evaluated. The header renders at once; the rules stream in.
 */
export default function CheckoutSettingsPage() {
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={960}>
        <PageHeader
          back={SETTINGS_CRUMB}
          title="Checkout"
          description="Address verification and autocomplete, apps that validate carts, and rules on the shipping address and the cart: an error stops checkout with your message under the field, a warning only shows it."
        />
        <Suspense fallback={<CardSkeleton lines={6} />}>
          <AddressApps />
        </Suspense>
        <Suspense fallback={<CardSkeleton lines={5} />}>
          <Rules />
        </Suspense>
      </VStack>
    </Section>
  );
}

/** The apps checkout asks: verification, autocomplete and validation. */
async function AddressApps() {
  const client = await sessionClient();
  const [me, settings, verifying, suggesting, validating] = await Promise.all([
    requireStaff(),
    client.GET("/admin/checkout-settings"),
    loadAnsweringApps(client, ["address.verify"]),
    loadAnsweringApps(client, ["address.suggest", "address.resolve"]),
    loadAnsweringApps(client, ["checkout.validate"]),
  ]);
  if (isMissingScope(settings)) return null;
  const current = loaded(settings);
  const canWrite = me.scopes.includes("checkout_rules:write");
  return (
    <>
      <AppSettingCard
        setting="verification_app"
        current={current.verification_app ?? null}
        apps={verifying}
        canWrite={canWrite}
      />
      <AppSettingCard
        setting="autocomplete_app"
        current={current.autocomplete_app ?? null}
        apps={suggesting}
        canWrite={canWrite}
      />
      <ValidationAppsCard
        apps={current.validation_apps}
        names={Object.fromEntries(validating.map((a) => [a.value, a.label]))}
        canWrite={canWrite}
      />
    </>
  );
}

async function Rules() {
  const client = await sessionClient();
  const [me, rules, options] = await Promise.all([
    requireStaff(),
    client.GET("/admin/checkout-rules"),
    loadRuleOptions(client),
  ]);
  if (isMissingScope(rules)) {
    return (
      <Banner
        status="info"
        title="Your role can't see checkout rules"
        description="Ask the store owner for the checkout_rules:read scope."
      />
    );
  }
  return (
    <RulesCard
      rules={loaded(rules).items}
      options={options}
      canWrite={me.scopes.includes("checkout_rules:write")}
    />
  );
}
