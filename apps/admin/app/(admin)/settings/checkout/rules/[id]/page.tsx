import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Suspense } from "react";

import { PageHeader } from "@/components/page-header";
import { DetailSkeleton } from "@/components/skeletons";
import { formOf } from "@/lib/checkout-rules";
import { loadCurrencies, loaded, requireStaff, sessionClient } from "@/lib/hypha";

import { productSummaries } from "../../../../markets/price-lists/grid-data";
import { loadRuleOptions } from "../../data";
import { RuleEditor } from "../../rule-editor";

const BACK = [
  { label: "Settings", href: "/settings" },
  { label: "Checkout", href: "/settings/checkout" },
];

/** A checkout rule: its editor and the "Try an address" panel. */
export default async function RulePage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={1320}>
        <Suspense fallback={<DetailSkeleton back={BACK} />}>
          <Rule id={id} />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function Rule({ id }: { id: string }) {
  const client = await sessionClient();
  const [me, rule, options, currencies] = await Promise.all([
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
  return (
    <>
      <PageHeader title={rule.name} back={BACK} />
      <RuleEditor
        id={rule.id}
        initial={formOf(rule, currencies)}
        options={options}
        productNames={names}
        canWrite={me.scopes.includes("checkout_rules:write")}
      />
    </>
  );
}
