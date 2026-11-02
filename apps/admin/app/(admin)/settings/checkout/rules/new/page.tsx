import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Suspense } from "react";

import { PageHeader } from "@/components/page-header";
import { type SearchParams, one } from "@/components/resource-table/params";
import { FormSkeleton } from "@/components/skeletons";
import { blankRule, presetOf } from "@/lib/checkout-rules";
import { requireStaff, sessionClient } from "@/lib/hypha";

import { loadRuleOptions, loadTryFormat } from "../../data";
import { RuleEditor } from "../../rule-editor";

const BACK = [
  { label: "Settings", href: "/settings" },
  { label: "Checkout", href: "/settings/checkout" },
];

/** A new checkout rule, from a preset (`?preset=`) or blank. */
export default async function NewRulePage({
  searchParams,
}: {
  searchParams: Promise<SearchParams>;
}) {
  const preset = presetOf(one(await searchParams, "preset"));
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={1320}>
        <PageHeader
          title={preset ? `Add rule: ${preset.title}` : "Add checkout rule"}
          description={preset?.description ?? "Pick what must hold, and what the shopper is told."}
          back={BACK}
        />
        <Suspense fallback={<FormSkeleton cards={4} />}>
          <Form presetKey={preset?.key} />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function Form({ presetKey }: { presetKey: string | undefined }) {
  const client = await sessionClient();
  const [me, options] = await Promise.all([requireStaff(), loadRuleOptions(client)]);
  const initial = presetOf(presetKey)?.rule() ?? blankRule();
  const tryFormat = await loadTryFormat(client, initial, options);
  return (
    <RuleEditor
      initial={initial}
      options={options}
      tryFormat={tryFormat}
      productNames={{}}
      canWrite={me.scopes.includes("checkout_rules:write")}
    />
  );
}
