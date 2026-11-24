import { Banner } from "@astryxdesign/core/Banner";
import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Suspense } from "react";

import { PageHeader } from "@/components/page-header";
import { CardSkeleton } from "@/components/skeletons";
import { isMissingScope, loaded, requireStaff, sessionClient } from "@/lib/hypha";
import { getT } from "@/lib/i18n";
import { settingsCrumb } from "@/lib/navigation";

import { AppSettingCard } from "./app-setting-card";
import { loadAnsweringApps, loadRuleOptions } from "./data";
import { RulesCard } from "./rules-card";
import { ValidationAppsCard } from "./validation-apps-card";

/**
 * Settings → Checkout: the rules a cart must meet to be checked out, in the order
 * they're evaluated. The header renders at once; the rules stream in.
 */
export default async function CheckoutSettingsPage() {
  const t = await getT();
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={960}>
        <PageHeader
          back={settingsCrumb(t)}
          title={t("checkout.page_title")}
          description={t("checkout.page_description")}
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
  const [t, me, rules, options] = await Promise.all([
    getT(),
    requireStaff(),
    client.GET("/admin/checkout-rules"),
    loadRuleOptions(client),
  ]);
  if (isMissingScope(rules)) {
    return (
      <Banner
        status="info"
        title={t("checkout.rules_no_access_title")}
        description={t("checkout.rules_no_access_description")}
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
