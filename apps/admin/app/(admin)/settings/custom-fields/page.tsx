import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";

import { PageHeader } from "@/components/page-header";
import { isOwnerType } from "@/lib/custom-fields";
import { loaded, requireStaff, sessionClient } from "@/lib/hypha";
import { SETTINGS_CRUMB, hasScope } from "@/lib/navigation";

import { DefinitionsCard } from "./definitions";

/**
 * Settings → Custom fields: per owner type, the fields its pages show. The merchant's
 * own can be added, changed, reordered and deleted; apps' and spores' are listed
 * read-only, named by their app.
 */
export default async function CustomFieldsSettingsPage({
  searchParams,
}: {
  searchParams: Promise<Record<string, string | string[] | undefined>>;
}) {
  const query = await searchParams;
  const owner =
    typeof query.owner === "string" && isOwnerType(query.owner) ? query.owner : "product";
  const client = await sessionClient();
  const [definitions, suggestions, me] = await Promise.all([
    client
      .GET("/admin/custom-field-definitions", { params: { query: { owner_type: owner } } })
      .then(loaded),
    // The well-known fields the API suggests for this owner type.
    client
      .GET("/admin/custom-field-definitions/suggestions", {
        params: { query: { owner_type: owner } },
      })
      .then(loaded),
    requireStaff(),
  ]);

  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={1100}>
        <PageHeader
          back={SETTINGS_CRUMB}
          title="Custom fields"
          description="Your own data on products, variants, categories, customers and orders: each field has a type, rules for its values, and a place on its record's page. Apps add fields of their own."
        />
        <DefinitionsCard
          key={owner}
          owner={owner}
          definitions={definitions.items.toSorted((a, b) => a.position - b.position)}
          suggestions={suggestions.items.filter((s) => !s.existing)}
          canWrite={hasScope(me.scopes, "custom_fields:write")}
        />
      </VStack>
    </Section>
  );
}
