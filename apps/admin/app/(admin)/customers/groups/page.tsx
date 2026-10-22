import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Suspense } from "react";

import { PageHeader } from "@/components/page-header";
import { CardSkeleton } from "@/components/skeletons";
import { loaded, requireStaff, sessionClient } from "@/lib/hypha";
import { hasScope } from "@/lib/navigation";

import { GroupsCard } from "./groups";

/** Customers → Groups: the header renders at once; the groups stream in. */
export default async function CustomerGroupsPage() {
  const me = await requireStaff();
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={960}>
        <PageHeader
          title="Customer groups"
          back={{ label: "Customers", href: "/customers" }}
          description="Named sets of customers with a stable ID: what group prices and payment methods will point at. Labels are for finding customers; groups are for rules."
        />
        <Suspense fallback={<CardSkeleton lines={6} />}>
          <Groups canWrite={hasScope(me.scopes, "customers:write")} />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function Groups({ canWrite }: { canWrite: boolean }) {
  const client = await sessionClient();
  const groups = await client.GET("/admin/customer-groups").then(loaded);
  return <GroupsCard groups={groups.items} canWrite={canWrite} />;
}
