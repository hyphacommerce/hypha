import { Button } from "@astryxdesign/core/Button";
import { VStack } from "@astryxdesign/core/Layout";
import { Section } from "@astryxdesign/core/Section";
import { Suspense } from "react";

import { PageHeader } from "@/components/page-header";
import { type SearchParams, one, oneOf } from "@/components/resource-table/params";
import { listPage, pageQuery } from "@/components/resource-table/server";
import { requireStaff, sessionClient } from "@/lib/hypha";
import { hasScope } from "@/lib/navigation";

import { PriceListsTable } from "./price-lists-table";

const KINDS = ["sale", "override"] as const;
const PHASES = ["draft", "scheduled", "active", "expired"] as const;
const SORTS = ["newest", "oldest", "name_asc", "name_desc"] as const;

/**
 * Every price list: its kind, where it stands now, its window, who it's for, the markets
 * using it and how many fixed prices it has. The header and the table's toolbar render
 * at once; the lists stream in.
 */
export default async function PriceListsPage({
  searchParams,
}: {
  searchParams: Promise<SearchParams>;
}) {
  const [params, me] = await Promise.all([searchParams, requireStaff()]);
  return (
    <Section padding={6}>
      <VStack gap={5}>
        <PageHeader
          title="Price lists"
          description="Prices that differ from the base prices: a market's own, a sale, wholesale. A market can use several; shoppers get the lowest live price among them."
          back={{ label: "Markets", href: "/markets" }}
          actions={
            hasScope(me.scopes, "pricing:write") && (
              <Button label="New price list" variant="primary" href="/markets/price-lists/new" />
            )
          }
        />
        <Suspense fallback={<PriceListsTable />}>
          <PriceListRows params={params} />
        </Suspense>
      </VStack>
    </Section>
  );
}

async function PriceListRows({ params }: { params: SearchParams }) {
  const client = await sessionClient();
  const [result, markets, groups] = await Promise.all([
    client.GET("/admin/price-lists", {
      params: {
        query: {
          ...pageQuery(params),
          kind: oneOf(params, "kind", KINDS),
          status: oneOf(params, "status", PHASES),
          market: one(params, "market"),
          group: one(params, "group"),
          sort: oneOf(params, "sort", SORTS),
        },
      },
    }),
    // Without `markets:read` the lists show their markets by ID.
    client.GET("/admin/markets"),
    // Without `customers:read` their audiences by ID.
    client.GET("/admin/customer-groups"),
  ]);
  return (
    <PriceListsTable
      page={listPage(result, "/markets/price-lists", params)}
      markets={Object.fromEntries((markets.data?.items ?? []).map((m) => [m.id, m.name]))}
      groups={Object.fromEntries((groups.data?.items ?? []).map((g) => [g.id, g.name]))}
    />
  );
}
