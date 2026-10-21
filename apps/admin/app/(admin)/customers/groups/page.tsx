import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";

import { PageHeader } from "@/components/page-header";
import { loaded, requireStaff, sessionClient } from "@/lib/hypha";
import { hasScope } from "@/lib/navigation";

import { GroupsCard } from "./groups";

/** Customers → Groups (design note 0032, §6). */
export default async function CustomerGroupsPage() {
  const client = await sessionClient();
  const [groups, me] = await Promise.all([
    client.GET("/admin/customer-groups").then(loaded),
    requireStaff(),
  ]);
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={960}>
        <PageHeader
          title="Customer groups"
          back={{ label: "Customers", href: "/customers" }}
          description="Named sets of customers with a stable ID: what group prices and payment methods will point at. Labels are for finding customers; groups are for rules."
        />
        <GroupsCard groups={groups.items} canWrite={hasScope(me.scopes, "customers:write")} />
      </VStack>
    </Section>
  );
}
