import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Suspense } from "react";

import { type Crumb, PageHeader } from "@/components/page-header";
import { type SearchParams, one } from "@/components/resource-table/params";
import { FormSkeleton } from "@/components/skeletons";
import { blankRule, presetOf } from "@/lib/checkout-rules";
import { requireStaff, sessionClient } from "@/lib/hypha";
import { type T, getT } from "@/lib/i18n";
import { settingsCrumb } from "@/lib/navigation";

import { loadRuleOptions, loadTryFormat } from "../../data";
import { RuleEditor } from "../../rule-editor";

const back = (t: T): Crumb[] => [
  settingsCrumb(t),
  { label: t("checkout.page_title"), href: "/settings/checkout" },
];

/** A new checkout rule, from a preset (`?preset=`) or blank. */
export default async function NewRulePage({
  searchParams,
}: {
  searchParams: Promise<SearchParams>;
}) {
  const [query, t] = await Promise.all([searchParams, getT()]);
  const preset = presetOf(one(query, "preset"));
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={1320}>
        <PageHeader
          title={
            preset
              ? t("checkout.new_title_preset", {
                  preset: t(`checkout.preset_${preset.key}_title`),
                })
              : t("checkout.new_title")
          }
          description={
            preset ? t(`checkout.preset_${preset.key}_description`) : t("checkout.new_description")
          }
          back={back(t)}
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
