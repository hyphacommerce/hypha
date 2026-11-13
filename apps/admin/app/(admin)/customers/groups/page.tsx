import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Suspense } from "react";

import { PageHeader } from "@/components/page-header";
import { CardSkeleton } from "@/components/skeletons";
import { loaded, requireStaff, sessionClient } from "@/lib/hypha";
import { getT } from "@/lib/i18n";
import { hasScope } from "@/lib/navigation";

import { GroupsCard } from "./groups";

/** Customers → Groups: the header renders at once; the groups stream in. */
export default async function CustomerGroupsPage() {
  const [me, t] = await Promise.all([requireStaff(), getT()]);
  return (
    <Section padding={6}>
      <VStack gap={5} maxWidth={960}>
        <PageHeader
          title={t("customers.groups_page_title")}
          back={{ label: t("common.customers"), href: "/customers" }}
          description={t("customers.groups_page_description")}
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
