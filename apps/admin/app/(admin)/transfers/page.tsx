import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Suspense } from "react";

import { PageHeader } from "@/components/page-header";
import { type SearchParams, one } from "@/components/resource-table/params";
import { listPage, pageQuery } from "@/components/resource-table/server";
import { locationNames, requireStaff, sessionClient } from "@/lib/hypha";
import { hasScope } from "@/lib/navigation";

import { NewTransfer } from "./new-transfer";
import { TransfersTable } from "./transfers-table";

const STATUSES = ["draft", "in_transit", "received", "cancelled"] as const;

function pick<T extends string>(value: string | undefined, allowed: readonly T[]): T | undefined {
  return allowed.find((a) => a === value);
}

/**
 * Stock moving between the store's locations: where each transfer goes from and to, how
 * far it got and when it's due. The header and the table's toolbar render at once; the
 * rows stream in.
 */
export default async function TransfersPage({
  searchParams,
}: {
  searchParams: Promise<SearchParams>;
}) {
  const [params, me] = await Promise.all([searchParams, requireStaff()]);
  const canWrite = hasScope(me.scopes, "inventory:write");
  return (
    <Section padding={6}>
      <VStack gap={5}>
        <PageHeader
          title="Transfers"
          description="Stock moved between your locations: what left, what's on its way, what arrived."
          actions={canWrite && <NewTransfer />}
        />
        <Suspense fallback={<TransfersTable canWrite={canWrite} />}>
          <Rows params={params} canWrite={canWrite} />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function Rows({ params, canWrite }: { params: SearchParams; canWrite: boolean }) {
  const client = await sessionClient();
  const [result, locations] = await Promise.all([
    client.GET("/admin/transfers", {
      params: {
        query: {
          ...pageQuery(params),
          status: pick(one(params, "status"), STATUSES),
          origin: one(params, "origin"),
          destination: one(params, "destination"),
        },
      },
    }),
    locationNames(client),
  ]);
  return (
    <TransfersTable
      page={await listPage(result, "/transfers", params)}
      locations={Object.fromEntries(locations)}
      canWrite={canWrite}
    />
  );
}
