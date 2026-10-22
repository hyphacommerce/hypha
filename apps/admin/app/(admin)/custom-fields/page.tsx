import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Suspense } from "react";

import { PageHeader } from "@/components/page-header";
import { CardSkeleton } from "@/components/skeletons";
import { type OwnerType, isOwnerType } from "@/lib/custom-fields";
import { loaded, requireStaff, sessionClient } from "@/lib/hypha";
import { hasScope } from "@/lib/navigation";

import { DefinitionsCard } from "./definitions";
import { OwnerTabs } from "./owner-tabs";

/**
 * Content → Custom fields: per owner type, the fields its pages show. The merchant's
 * own can be added, changed, reordered and deleted; apps' and spores' are listed
 * read-only, named by their app. The header and the tabs render at once; the chosen
 * owner type's fields stream in.
 */
export default async function CustomFieldsPage({
  searchParams,
}: {
  searchParams: Promise<Record<string, string | string[] | undefined>>;
}) {
  const query = await searchParams;
  const owner =
    typeof query.owner === "string" && isOwnerType(query.owner) ? query.owner : "product";

  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={1100}>
        <PageHeader
          title="Custom fields"
          description="Your own data on products, variants, categories, customers and orders: each field has a type, rules for its values, and a place on its record's page. Apps add fields of their own."
        />
        <OwnerTabs owner={owner}>
          <Suspense fallback={<CardSkeleton lines={4} />}>
            <Definitions owner={owner} />
          </Suspense>
        </OwnerTabs>
      </VStack>
    </Section>
  );
}

/** The owner type's fields and the well-known ones the API suggests for it. */
async function Definitions({ owner }: { owner: OwnerType }) {
  const client = await sessionClient();
  const [definitions, suggestions, me] = await Promise.all([
    client
      .GET("/admin/custom-field-definitions", { params: { query: { owner_type: owner } } })
      .then(loaded),
    client
      .GET("/admin/custom-field-definitions/suggestions", {
        params: { query: { owner_type: owner } },
      })
      .then(loaded),
    requireStaff(),
  ]);
  return (
    <DefinitionsCard
      key={owner}
      owner={owner}
      definitions={definitions.items.toSorted((a, b) => a.position - b.position)}
      suggestions={suggestions.items.filter((s) => !s.existing)}
      canWrite={hasScope(me.scopes, "custom_fields:write")}
    />
  );
}
