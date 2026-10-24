import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Suspense } from "react";

import { PageHeader } from "@/components/page-header";
import { type SearchParams, one } from "@/components/resource-table/params";
import { FormSkeleton } from "@/components/skeletons";
import { templateOf } from "@/lib/discounts/templates";
import { sessionClient } from "@/lib/hypha";

import { loadLive, loadOptions } from "../data";
import { NewDiscount } from "../discount-editor";

const BACK = [{ label: "Discounts", href: "/discounts" }];

/**
 * A new discount: a template (`?template=` picks one), then the form. The header
 * renders at once; the form waits for the store's markets, channels and categories.
 */
export default async function NewDiscountPage({
  searchParams,
}: {
  searchParams: Promise<SearchParams>;
}) {
  const params = await searchParams;
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={1320}>
        <PageHeader
          title="Create discount"
          description="Start from what it does; every setting can be changed in the form."
          back={BACK}
        />
        <Suspense fallback={<FormSkeleton cards={4} />}>
          <Form template={one(params, "template")} />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function Form({ template }: { template: string | undefined }) {
  const client = await sessionClient();
  const [options, live] = await Promise.all([loadOptions(client), loadLive(client)]);
  // From the server, so the browser judges the summary's phase at the same instant. A
  // server component renders once per request, so reading the clock here is fine.
  // oxlint-disable-next-line react/purity
  const now = Date.now();
  return (
    <NewDiscount
      options={options}
      names={{}}
      live={live}
      now={now}
      initial={templateOf(template ?? "")?.key ?? null}
    />
  );
}
