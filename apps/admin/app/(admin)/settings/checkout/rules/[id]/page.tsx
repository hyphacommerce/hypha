import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Suspense } from "react";

import { type Crumb, PageHeader } from "@/components/page-header";
import { DetailSkeleton } from "@/components/skeletons";
import { formOf } from "@/lib/checkout-rules";
import { loadCurrencies, loaded, requireStaff, sessionClient } from "@/lib/hypha";
import { type T, getT } from "@/lib/i18n";
import { settingsCrumb } from "@/lib/navigation";

import { productSummaries } from "../../../../markets/price-lists/grid-data";
import { loadRuleOptions, loadTryFormat } from "../../data";
import { RuleEditor } from "../../rule-editor";

const back = (t: T): Crumb[] => [
  settingsCrumb(t),
  { label: t("checkout.page_title"), href: "/settings/checkout" },
];

/** A checkout rule: its editor and the "Try an address" panel. */
export default async function RulePage({ params }: { params: Promise<{ id: string }> }) {
  const [{ id }, t] = await Promise.all([params, getT()]);
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={1320}>
        <Suspense fallback={<DetailSkeleton back={back(t)} />}>
          <Rule id={id} />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function Rule({ id }: { id: string }) {
  const client = await sessionClient();
  const [t, me, rule, options, currencies] = await Promise.all([
    getT(),
    requireStaff(),
    client.GET("/admin/checkout-rules/{id}", { params: { path: { id } } }).then(loaded),
    loadRuleOptions(client),
    loadCurrencies(),
  ]);
  const products = [
    ...new Set(rule.conditions.flatMap((c) => ("products" in c ? (c.products ?? []) : []))),
  ];
  const names = await productSummaries(client, products).then(
    (found) => Object.fromEntries(found.map((p) => [p.id, p.title])),
    () => ({}),
  );
  const initial = formOf(rule, currencies);
  const tryFormat = await loadTryFormat(client, initial, options);
  return (
    <>
      <PageHeader title={rule.name} back={back(t)} />
      <RuleEditor
        id={rule.id}
        initial={initial}
        options={options}
        tryFormat={tryFormat}
        productNames={names}
        canWrite={me.scopes.includes("checkout_rules:write")}
      />
    </>
  );
}
